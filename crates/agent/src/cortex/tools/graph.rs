use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use crate::ops::graph::GraphTool as InnerGraphTool;
use crate::ops::graph::GraphDirection;

use serde_json::{json, Value};
use std::sync::Arc;
use crate::project::context::RepoContext;

pub struct InspectGraphTool {
    inner: Arc<InnerGraphTool>,
}

impl InspectGraphTool {
    pub fn new(inner: Arc<InnerGraphTool>, _ctx: Arc<RepoContext>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for InspectGraphTool {
    fn name(&self) -> &str {
        "inspect_graph"
    }

    fn description(&self) -> &str {
        "Explore the code graph to understand relationships (calls, imports, definitions). You can provide a specific Node ID OR a name/keyword (e.g., 'User', 'auth_flow') which will be resolved automatically."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "The Node ID or name to start from."
                },
                "file_filter": {
                    "type": "string",
                    "description": "Optional file path filter (e.g., 'cli/src/commands' or 'ask.rs'). Matches if file path contains this string. Use this to narrow down results when multiple symbols have the same name."
                },
                "kind": {
                    "type": "string",
                    "enum": ["file", "symbol", "chunk"],
                    "description": "Optional. The type of node to look for. Only use this if you need to distinguish between a file and a symbol with the same name."
                },
                "relation": {
                    "type": "string",
                    "enum": ["incoming", "outgoing", "both"],
                    "description": "Direction of traversal. Use 'outgoing' to see what this node uses (definitions, calls). Use 'incoming' to see what uses this node (callers, importers).",
                    "default": "outgoing"
                },
                "max_hops": {
                    "type": "integer",
                    "default": 1
                }
            },
            "required": ["node"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let node_query = args["node"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'node' argument"))?;
        let file_filter = args["file_filter"].as_str();
        let _kind = args["kind"].as_str(); // Ignored for now, handled by GraphTool heuristics
        let relation = args["relation"].as_str().unwrap_or("outgoing");
        let max_hops = args["max_hops"].as_u64().unwrap_or(1) as usize;

        let direction = match relation {
            "incoming" => GraphDirection::In,
            "both" => GraphDirection::Both,
            _ => GraphDirection::Out,
        };

        match self.inner.graph(node_query, direction, max_hops, file_filter).await {
            Ok(result) => {
                // Check for disambiguation
                if let Some(candidates) = result.candidates {
                    let mut response = format!(
                        "Found {} symbols matching '{}':\n\n", 
                        candidates.len(), 
                        node_query
                    );
                    
                    for (i, cand) in candidates.iter().enumerate() {
                        response.push_str(&format!(
                            "{}. {} ({})\n   File: {}\n   ID: {}\n\n",
                            i + 1, cand.label, cand.kind, cand.file_path, cand.id
                        ));
                    }
                    
                    response.push_str(&format!(
                        "Please call 'inspect_graph' again with:\n\
                         - The specific node ID from above, OR\n\
                         - Use 'file_filter' to narrow results\n\
                         Example: {{\"node\": \"{}\", \"relation\": \"{}\"}}", 
                        candidates[0].id, relation
                    ));
                    
                    return Ok(response);
                }
                
                // Success - return graph
                let json_output = serde_json::to_string_pretty(&result.subgraph)?;
                Ok(json_output)
            }
            Err(e) => {
                if e.to_string().contains("not found") {
                     Ok(format!("Node '{}' not found in the graph. Try searching for the symbol first using 'search_code' to find the correct name or ID.", node_query))
                } else {
                    Err(e)
                }
            }
        }
    }
}

