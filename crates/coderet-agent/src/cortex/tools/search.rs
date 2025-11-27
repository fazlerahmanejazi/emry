use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use coderet_tools::search::Search as InnerSearchTool;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct SearchCodeTool {
    inner: Arc<InnerSearchTool>,
}

impl SearchCodeTool {
    pub fn new(inner: Arc<InnerSearchTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for SearchCodeTool {
    fn name(&self) -> &str {
        "search_code"
    }

    fn description(&self) -> &str {
        "Search the codebase for code snippets using semantic and lexical search. Use this to find relevant code when you have a general idea or keywords."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query (e.g., 'feature name', 'error message')"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max number of results (default: 10)",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;

        let results = self.inner.search(query, limit).await?;
        
        if results.chunks.is_empty() {
            return Ok("No results found.".to_string());
        }

        let mut out = String::new();
        for (i, res) in results.chunks.iter().enumerate() {
            out.push_str(&format!(
                "Result {}:\nFile: {}\nLine: {}\nScore: {:.2}\nContent:\n{}\n\n",
                i + 1,
                res.chunk.file_path.display(),
                res.chunk.start_line,
                res.score,
                res.chunk.content.trim()
            ));
        }
        Ok(out)
    }
}
