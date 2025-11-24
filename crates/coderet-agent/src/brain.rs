use crate::llm::{Message, ModelProvider};
use anyhow::Result;
use coderet_index::manager::IndexManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentState {
    Thinking,
    ToolCall(String, String), // Tool Name, Args (JSON)
    ToolOutput(String),       // Output
    FinalAnswer(String),
}

pub struct Brain<P: ModelProvider> {
    provider: P,
    index_manager: Arc<IndexManager>,
    history: Vec<Message>,
    max_steps: usize,
}

impl<P: ModelProvider> Brain<P> {
    pub fn new(provider: P, index_manager: Arc<IndexManager>) -> Self {
        Self {
            provider,
            index_manager,
            history: Vec::new(),
            max_steps: 10,
        }
    }

    pub async fn ask(&mut self, question: &str) -> Result<String> {
        self.history.push(Message {
            role: "user".to_string(),
            content: question.to_string(),
        });

        let mut steps = 0;
        while steps < self.max_steps {
            let state = self.decide_next_step().await?;
            match state {
                AgentState::Thinking => {
                    // Should not happen if decide_next_step returns a concrete action
                    continue;
                }
                AgentState::ToolCall(name, args) => {
                    let output = self.execute_tool(&name, &args).await?;
                    self.history.push(Message {
                        role: "assistant".to_string(),
                        content: format!("Tool Call: {} {}", name, args),
                    });
                    self.history.push(Message {
                        role: "user".to_string(), // Or "tool" role if supported
                        content: format!("Tool Output: {}", output),
                    });
                }
                AgentState::ToolOutput(_) => {
                    // Internal state
                }
                AgentState::FinalAnswer(answer) => {
                    self.history.push(Message {
                        role: "assistant".to_string(),
                        content: answer.clone(),
                    });
                    return Ok(answer);
                }
            }
            steps += 1;
        }

        Ok("I reached the maximum number of steps without a final answer.".to_string())
    }

    async fn decide_next_step(&self) -> Result<AgentState> {
        // Construct system prompt with tools
        let system_prompt = r#"You are a code retrieval assistant. You have access to the following tools:
- search(query: str): Search the codebase for relevant code chunks.
- read_file(path: str): Read the content of a file.
- list_files(path: str): List files in a directory.

Response Format:
If you want to call a tool, respond with:
TOOL: <tool_name> <json_args>

If you have the final answer, respond with:
ANSWER: <your answer>
"#;

        let mut messages = vec![Message {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        }];
        messages.extend(self.history.clone());

        let response = self.provider.chat(&messages).await?;

        // Parse response
        if let Some(stripped) = response.strip_prefix("TOOL: ") {
            let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
            if parts.len() == 2 {
                return Ok(AgentState::ToolCall(
                    parts[0].to_string(),
                    parts[1].to_string(),
                ));
            }
        } else if let Some(stripped) = response.strip_prefix("ANSWER: ") {
            return Ok(AgentState::FinalAnswer(stripped.to_string()));
        }

        // Fallback: treat as answer if no prefix, or retry?
        // For now, treat as answer
        Ok(AgentState::FinalAnswer(response))
    }

    async fn execute_tool(&self, name: &str, args: &str) -> Result<String> {
        match name {
            "search" => {
                // Parse args: {"query": "..."}
                let json: serde_json::Value = serde_json::from_str(args)?;
                let query = json["query"].as_str().unwrap_or("");
                let results = self.index_manager.search(query, 5).await?;
                let mut output = String::new();
                for (score, chunk) in results {
                    output.push_str(&format!(
                        "- [{:.2}] {}:{}: {}\n",
                        score,
                        chunk.file_path.display(),
                        chunk.start_line,
                        chunk.content.trim()
                    ));
                }
                Ok(output)
            }
            "read_file" => {
                Ok("Reading file...".to_string()) // Placeholder
            }
            "list_files" => {
                Ok("Listing files...".to_string()) // Placeholder
            }
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }
}
