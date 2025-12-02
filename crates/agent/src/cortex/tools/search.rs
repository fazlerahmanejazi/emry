use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use crate::ops::search::Search as InnerSearchTool;
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

        // Use smart search by default for the agent
        let context_graph = self.inner.search_with_context(query, limit, true).await?;
        
        if context_graph.anchors.is_empty() {
            return Ok("No results found.".to_string());
        }

        let mut out = String::new();
        
        // Use shared grouping logic
        let grouped = context_graph.group_by_symbol();

        // Output Symbol Groups
        for group in grouped.groups {
            let content = emry_core::models::ScoredChunk::concatenate_chunks(&group.anchors);
            let start_line = group.anchors.iter().map(|c| c.chunk.start_line).min().unwrap_or(0);
            let end_line = group.anchors.iter().map(|c| c.chunk.end_line).max().unwrap_or(0);

            out.push_str(&format!("Symbol: {} ({})\n", group.symbol.name, group.symbol.kind));
            out.push_str(&format!("  File: {}:{}-{}\n", group.symbol.file_path.display(), start_line, end_line));
            
            if !group.calls.is_empty() {
                out.push_str("  Calls: ");
                for (j, call) in group.calls.iter().enumerate() {
                    if j > 0 { out.push_str(", "); }
                    out.push_str(&call.name);
                }
                out.push_str("\n");
            }

            out.push_str("  Content:\n");
            out.push_str(&format!("    {}\n", content.trim().replace('\n', "\n    ")));
            out.push_str("\n");
        }

        // Output Unassigned Anchors
        if !grouped.unassigned.is_empty() {
            out.push_str("Other Matches:\n");
            for anchor in grouped.unassigned {
                out.push_str(&format!(
                    "  File: {}\n  Line {}-{}: {:.2}\n  Content:\n    {}\n\n",
                    anchor.chunk.file_path.display(),
                    anchor.chunk.start_line,
                    anchor.chunk.end_line,
                    anchor.score,
                    anchor.chunk.content.trim().replace('\n', "\n    ")
                ));
            }
        }
        Ok(out)
    }
}
