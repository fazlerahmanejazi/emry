pub mod context;
pub mod tool;
pub mod tools;
pub mod prompts;

use crate::cortex::context::AgentContext;
use crate::llm::OpenAIProvider;
use anyhow::Result;

pub struct Cortex {
    pub ctx: AgentContext,
    pub llm: OpenAIProvider,
}

impl Cortex {
    pub fn new(ctx: AgentContext, llm: OpenAIProvider) -> Self {
        Self { ctx, llm }
    }

    pub async fn run(&mut self, query: &str) -> Result<String> {
        self.ctx.history.clear();
        let max_steps = 10;
        
        // 1. Initialize Messages
        let mut messages = Vec::new();
        
        // System Message
        let mut system_prompt = crate::cortex::prompts::SYSTEM_PROMPT.to_string();
        
        // Add workspace context
        system_prompt.push_str(&format!(
            "\n\n# WORKSPACE CONTEXT\n\
             Workspace Root: {}\n\
             All file operations must use paths within this workspace. \
             Use '.' to refer to the workspace root or relative paths like 'src/module'.\n",
            self.ctx.repo_context.root.display()
        ));
        
        system_prompt.push_str("\n\n# AVAILABLE TOOLS\n");
        for tool in self.ctx.tools.values() {
            system_prompt.push_str(&format!("- {}: {}\n  Schema: {}\n", tool.name(), tool.description(), tool.schema()));
        }
        messages.push(crate::llm::Message {
            role: "system".to_string(),
            content: system_prompt,
        });
        
        // Initial User Message (Query + Memory)
        let mut user_content = format!("## Current Task\nQuery: {}\n\n", query);
        if !self.ctx.memory.is_empty() {
            user_content.push_str("## Memory\n");
            for item in &self.ctx.memory {
                user_content.push_str(&format!("- {}\n", item));
            }
        }
        messages.push(crate::llm::Message {
            role: "user".to_string(),
            content: user_content,
        });

        // 2. The Loop
        for step_count in 1..=max_steps {
            // a. Define Schema
            let schema = serde_json::json!({
                "type": "object",
                "properties": {
                    "thought": { "type": "string", "description": "Reasoning for the next step" },
                    "action": { "type": "string", "description": "Name of the tool to execute" },
                    "args": { "type": "object", "description": "Arguments for the tool" }
                },
                "required": ["thought", "action", "args"],
                "additionalProperties": false
            });

            // b. Call LLM
            let response = self.llm.chat_with_schema(
                &messages,
                crate::llm::JsonSchemaSpec {
                    name: "cortex_step".to_string(),
                    schema,
                }
            ).await?;
            
            // c. Parse Response
            let step_data: serde_json::Value = serde_json::from_str(&response)
                .or_else(|_| serde_json::from_str(response.trim()))
                .map_err(|e| anyhow::anyhow!("Failed to parse LLM response: {}", e))?;
                
            let thought = step_data["thought"].as_str().unwrap_or("").to_string();
            let action = step_data["action"].as_str().unwrap_or("").to_string();
            let args = step_data["args"].clone();
            
            // Record Assistant Message
            messages.push(crate::llm::Message {
                role: "assistant".to_string(),
                content: response.clone(),
            });

            // d. Check for Final Answer
            if action == "final_answer" {
                let answer = &args["answer"];
                return Ok(if answer.is_string() {
                    answer.as_str().unwrap_or("").to_string()
                } else {
                    serde_json::to_string_pretty(answer).unwrap_or_else(|_| "".to_string())
                });
            }
            
            // e. Execute Tool
            let tool_name = action.clone();
            let tool_result = if let Some(tool) = self.ctx.tools.get(&tool_name) {
                match tool.execute(args.clone()).await {
                    Ok(res) => res,
                    Err(e) => format!("Error executing tool '{}': {}", tool_name, e),
                }
            } else {
                format!("Tool '{}' not found. Available tools: {:?}", tool_name, self.ctx.tools.keys())
            };

            // Record User Message (Observation)
            messages.push(crate::llm::Message {
                role: "user".to_string(),
                content: format!("Observation: {}", tool_result),
            });

            // f. Update History (Internal Context)
            self.ctx.add_step(crate::cortex::context::Step {
                step_id: step_count,
                thought: thought.clone(),
                action: action.clone(),
                args: args.clone(),
                observation: tool_result.clone(),
                error: None,
            });
            
            // g. Check for Max Steps
            if self.ctx.history.len() >= max_steps {
                return Ok("Reached maximum steps without final answer.".to_string());
            }
        }
        
        Ok("Max steps reached without final answer.".to_string())
    }
}
