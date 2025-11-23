use anyhow::Result;
use serde_json::{json, Value};
use crate::agent::tools::ToolRegistry;
use crate::agent::llm::LLMClient;
use crate::config::SearchMode;

const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("prompt.txt");

pub struct Agent {
    llm: LLMClient,
    tools: ToolRegistry,
}

#[derive(Clone, Debug, Default)]
pub struct AgentOptions {
    pub top: Option<usize>,
    pub mode: Option<SearchMode>,
    pub lang: Option<String>,
    pub path: Option<String>,
    pub with_summaries: bool,
    pub depth: AgentDepth,
}

#[derive(Clone, Debug, Default)]
pub enum AgentDepth {
    Shallow,
    #[default]
    Default,
    Deep,
}

impl Agent {
    pub fn new(llm: LLMClient, tools: ToolRegistry) -> Self {
        Self { llm, tools }
    }

    pub async fn ask(&self, query: &str, opts: AgentOptions) -> Result<String> {
        // Simple ReAct loop with tool-first enforcement and retry if no tool is called.
        let system_prompt = SYSTEM_PROMPT_TEMPLATE.replace("{tools}", &self.list_tools_description());

        let mut messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": format!("{}\n\nUser options: top={:?}, mode={:?}, lang={:?}, path={:?}, with_summaries={}", query, opts.top, opts.mode, opts.lang, opts.path, opts.with_summaries)}),
        ];

        let mut used_tool = false;

        let max_steps = match opts.depth {
            AgentDepth::Shallow => 3,
            AgentDepth::Default => 6,
            AgentDepth::Deep => 12,
        };

        for _ in 0..max_steps {
            let response = self.llm.chat(messages.clone()).await?;

            if let Some((tool_name, args)) = self.parse_tool_call(&response) {
                println!("Agent Thought: Using tool {} with args {:?}", tool_name, args);

                messages.push(json!({"role": "assistant", "content": response}));

                let tool_result = if let Some(tool) = self.tools.get(&tool_name) {
                    match tool.call(self.merge_defaults(&tool_name, args, &opts)).await {
                        Ok(res) => res,
                        Err(e) => format!("Error: {}", e),
                    }
                } else {
                    format!("Error: Tool '{}' not found.", tool_name)
                };

                used_tool = true;
                messages.push(json!({"role": "user", "content": format!("Tool Output: {}", tool_result)}));
            } else {
                if !used_tool {
                    // Enforce tool-first: nudge the model once to emit a tool call.
                    messages.push(json!({"role": "assistant", "content": response}));
                    messages.push(json!({"role": "user", "content": "No tool call seen. Respond with a JSON tool call only."}));
                    continue;
                }
                return Ok(response);
            }
        }

        Ok("I reached the maximum number of steps without a final answer.".to_string())
    }

    fn list_tools_description(&self) -> String {
        let mut desc = String::new();
        for name in self.tools.list_tools() {
            if let Some(tool) = self.tools.get(&name) {
                desc.push_str(&format!("- {}: {}\n", name, tool.description()));
            }
        }
        desc
    }

    fn parse_tool_call(&self, response: &str) -> Option<(String, Value)> {
        // Scan for any balanced JSON object in the response and extract the first one with a "tool" field.
        let trimmed = response.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Fast path: whole message is JSON
        if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
            if let Some(res) = self.extract_tool_args(&val) {
                return Some(res);
            }
        }

        // Fallback: scan for balanced braces and try to parse each candidate.
        let mut starts = Vec::new();
        let mut depth = 0;
        for (idx, ch) in trimmed.char_indices() {
            match ch {
                '{' => {
                    if depth == 0 {
                        starts.push(idx);
                    }
                    depth += 1;
                }
                '}' => {
                    if depth > 0 {
                        depth -= 1;
                        if depth == 0 {
                            if let Some(start) = starts.pop() {
                                let candidate = &trimmed[start..=idx];
                                if let Ok(val) = serde_json::from_str::<Value>(candidate) {
                                    if let Some(res) = self.extract_tool_args(&val) {
                                        return Some(res);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        None
    }

    fn extract_tool_args(&self, val: &Value) -> Option<(String, Value)> {
        let obj = val.as_object()?;
        let tool = obj.get("tool")?.as_str()?.to_string();

        // Accept both { "tool": "...", "args": { ... } } and { "tool": "...", ... } shapes.
        // If args is missing, treat the remaining top-level fields as arguments.
        let args = match obj.get("args") {
            Some(Value::Object(map)) => Value::Object(map.clone()),
            Some(other) => {
                let mut map = serde_json::Map::new();
                map.insert("value".to_string(), other.clone());
                Value::Object(map)
            }
            None => {
                let mut map = serde_json::Map::new();
                for (k, v) in obj.iter() {
                    if k != "tool" {
                        map.insert(k.clone(), v.clone());
                    }
                }
                Value::Object(map)
            }
        };

        Some((tool, args))
    }

    fn merge_defaults(&self, tool_name: &str, mut args: Value, opts: &AgentOptions) -> Value {
        if tool_name != "search" {
            return args;
        }

        if !args.is_object() {
            args = json!({});
        }
        let args_obj = args.as_object_mut().unwrap();

        if args_obj.get("top").is_none() {
            if let Some(top) = opts.top {
                args_obj.insert("top".to_string(), json!(top));
            }
        }
        if args_obj.get("mode").is_none() {
            if let Some(mode) = opts.mode.clone() {
                args_obj.insert("mode".to_string(), json!(format!("{:?}", mode)));
            }
        }
        if args_obj.get("lang").is_none() {
            if let Some(lang) = opts.lang.clone() {
                args_obj.insert("lang".to_string(), json!(lang));
            }
        }
        if args_obj.get("path").is_none() {
            if let Some(path) = opts.path.clone() {
                args_obj.insert("path".to_string(), json!(path));
            }
        }
        if args_obj.get("with_summaries").is_none() && opts.with_summaries {
            args_obj.insert("with_summaries".to_string(), json!(true));
        }

        args
    }
}
