use anyhow::{anyhow, Result};
use coderet_config::AgentConfig;
use coderet_context::RepoContext;
use coderet_pipeline::manager::IndexManager;
use coderet_tools::{
    fs::FsTool,
    graph::GraphTool,
    search::Search,
    summaries::SummaryTool,
    FsToolTrait, GraphToolTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::llm::{Message, OpenAIProvider};
use crate::prompts::SYSTEM_PROMPT;

/// Represents a single thought-action-observation turn in the agent's loop.
#[derive(Debug, Clone, Serialize)]
pub struct AgentTurn {
    pub thought: String,
    pub tool_call: Option<ToolCall>,
    pub observation: Option<String>,
}

/// A parsed tool call from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_name: String,
    pub args: Value,
}

/// Represents the overall result of the agent's execution.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    pub final_answer: String,
    pub turns: Vec<AgentTurn>,
    pub metrics: AgentMetrics,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AgentMetrics {
    pub total_duration_ms: u128,
    pub llm_calls: usize,
    pub tool_calls: usize,
    pub total_steps: usize,
    pub tokens_used: usize,
}

pub struct AgentLoop {
    llm: OpenAIProvider,
    tools: AgentTools,
    config: AgentConfig,
}

/// Convenience bundle of all agent tools.
pub struct AgentTools {
    pub search: Search,
    pub summaries: SummaryTool,
    pub graph: GraphTool,
    pub fs: FsTool,
}

impl AgentTools {
    pub fn new(ctx: Arc<RepoContext>, manager: Arc<IndexManager>) -> Self {
        Self {
            search: Search::new(ctx.clone(), manager.clone()),
            summaries: SummaryTool::new(ctx.clone()),
            graph: GraphTool::new(ctx.clone()),
            fs: FsTool::new(ctx),
        }
    }
}

impl AgentLoop {
    pub fn new(ctx: Arc<RepoContext>, manager: Arc<IndexManager>) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("OPENAI_API_KEY not set for agent LLM"))?;
        let model = ctx.config.llm.model.clone();
        let api_base = ctx
            .config
            .llm
            .api_base
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let llm = OpenAIProvider::with_base(model, api_key, api_base, ctx.config.llm.timeout_secs)?;
        let tools = AgentTools::new(ctx.clone(), manager);

        Ok(Self {
            llm,
            tools,
            config: ctx.config.agent.clone(),
        })
    }

    pub async fn run(&self, question: &str, verbose: bool) -> Result<AgentResult> {
        let start_time = std::time::Instant::now();
        let mut messages: Vec<Message> = vec![
            Message {
                role: "system".to_string(),
                content: SYSTEM_PROMPT.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: format!("Question: {}", question),
            },
        ];
        let mut turns: Vec<AgentTurn> = Vec::new();
        let mut metrics = AgentMetrics::default();

        for step in 0..self.config.max_steps {
            metrics.total_steps = step + 1;
            if verbose {
                println!("\n--- Step {} ---", step + 1);
            }

            // Call LLM for thought and action
            let llm_response = timeout(
                Duration::from_secs(self.config.step_timeout_secs),
                self.llm.chat_with_limit(
                    &messages,
                    Some(self.config.max_tokens),
                ),
            )
            .await
            .map_err(|e| anyhow!("LLM call timed out: {}", e))??;
            metrics.llm_calls += 1;

            let current_turn = self.parse_llm_response(&llm_response)?;

            if verbose {
                println!("THOUGHT: {}", current_turn.thought);
                if let Some(tool_call) = &current_turn.tool_call {
                    println!("TOOL_CALL: {} {:?}", tool_call.tool_name, tool_call.args);
                }
            }

            messages.push(Message {
                role: "assistant".to_string(),
                content: llm_response.clone(),
            });

            if let Some(tool_call) = current_turn.tool_call {
                metrics.tool_calls += 1;
                let observation_content = timeout(
                    Duration::from_secs(self.config.step_timeout_secs),
                    self.execute_tool(&tool_call),
                )
                .await
                .map_err(|e| anyhow!("Tool execution timed out: {}", e))?;
                
                // Handle potential error from tool execution result
                let observation_content = observation_content.unwrap_or_else(|e| format!("Error: {:?}", e));
                
                if verbose {
                    println!("OBSERVATION: {}", observation_content);
                }

                messages.push(Message {
                    role: "user".to_string(),
                    content: format!("OBSERVATION: {}", observation_content),
                });

                turns.push(AgentTurn {
                    thought: current_turn.thought,
                    tool_call: Some(tool_call),
                    observation: Some(observation_content),
                });
            } else {
                // This must be a final answer
                turns.push(AgentTurn {
                    thought: current_turn.thought.clone(),
                    tool_call: None,
                    observation: None,
                });
                metrics.total_duration_ms = start_time.elapsed().as_millis();
                return Ok(AgentResult {
                    final_answer: current_turn.thought, // The last thought is the final answer
                    turns,
                    metrics,
                });
            }
        }

        metrics.total_duration_ms = start_time.elapsed().as_millis();
        Err(anyhow!(
            "Agent did not provide a final answer within {} steps.",
            self.config.max_steps
        ))
    }

    /// Parses the LLM's raw response into a structured AgentTurn.
    /// Expects THOUGHT: ... followed by optional TOOL_CALL: { ... } or FINAL_ANSWER: ...
    fn parse_llm_response(&self, response: &str) -> Result<AgentTurn> {
        let mut thought = String::new();
        let mut tool_call: Option<ToolCall> = None;
        let mut is_final_answer = false;

        for line in response.lines() {
            if line.starts_with("THOUGHT:") {
                thought.push_str(line.trim_start_matches("THOUGHT:").trim());
            } else if line.starts_with("TOOL_CALL:") {
                let json_str = line.trim_start_matches("TOOL_CALL:").trim();
                tool_call = Some(serde_json::from_str(json_str)?);
            } else if line.starts_with("FINAL_ANSWER:") {
                thought.push_str(line.trim_start_matches("FINAL_ANSWER:").trim());
                is_final_answer = true;
                break; // Final answer, stop processing
            } else {
                // If it's not a recognized prefix, it's part of the thought
                thought.push_str(line.trim());
            }
        }

        // If it's a final answer, the thought IS the final answer.
        // Otherwise, if there's no tool call, it's an implicit final answer (e.g., LLM just talked)
        if is_final_answer || (tool_call.is_none() && !thought.is_empty()) {
            return Ok(AgentTurn {
                thought: thought.trim().to_string(),
                tool_call: None,
                observation: None,
            });
        }

        Ok(AgentTurn {
            thought: thought.trim().to_string(),
            tool_call,
            observation: None, // Observation is populated after tool execution
        })
    }

    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<String> {
        match tool_call.tool_name.as_str() {
            "search" => {
                let query = tool_call.args["query"].as_str().unwrap_or_default();
                let limit = tool_call.args["limit"].as_u64().unwrap_or(10) as usize;
                let result = self.tools.search.search(query, limit).await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "explore" => {
                let path_str = tool_call.args["path"].as_str().unwrap_or(".");
                let path = PathBuf::from(path_str);
                let limit = tool_call.args["limit"].as_u64().unwrap_or(20) as usize;
                let result = self.tools.fs.list_files(&path, Some(limit))?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "outline" => {
                let path_str = tool_call.args["path"].as_str().unwrap_or_default();
                let path = PathBuf::from(path_str);
                let result = self.tools.fs.outline(&path)?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            "read" => {
                let path_str = tool_call.args["path"].as_str().unwrap_or_default();
                let path = PathBuf::from(path_str);
                let start_line = tool_call.args["start_line"].as_u64().unwrap_or(0) as usize;
                let end_line = tool_call.args["end_line"].as_u64().unwrap_or(0) as usize;
                let content = self.tools.fs.read_file_span(&path, start_line, end_line)?;
                Ok(serde_json::to_string_pretty(&serde_json::json!({"content": content}))?)
            }
            "graph" => {
                let symbol = tool_call.args["symbol"].as_str().unwrap_or_default();
                let direction_str = tool_call.args["direction"].as_str().unwrap_or("Out");
                let direction = match direction_str {
                    "In" => coderet_tools::graph::GraphDirection::In,
                    _ => coderet_tools::graph::GraphDirection::Out,
                };
                let max_hops = tool_call.args["max_hops"].as_u64().unwrap_or(3) as usize;
                let result = self.tools.graph.graph(symbol, direction, max_hops)?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
            _ => Err(anyhow!("Unknown tool: {}", tool_call.tool_name)),
        }
    }
}
