use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use coderet_tools::graph::GraphTool as InnerGraphTool;
use coderet_tools::graph::GraphDirection;

use serde_json::{json, Value};
use std::sync::Arc;
use coderet_context::RepoContext;

pub struct InspectGraphTool {
    inner: Arc<InnerGraphTool>,
    ctx: Arc<RepoContext>,
}

impl InspectGraphTool {
    pub fn new(inner: Arc<InnerGraphTool>, ctx: Arc<RepoContext>) -> Self {
        Self { inner, ctx }
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
        let kind = args["kind"].as_str();
        let relation = args["relation"].as_str().unwrap_or("outgoing");
        let max_hops = args["max_hops"].as_u64().unwrap_or(1) as usize;

        // 1. Resolve the node ID
        let node_id = {
            let graph = self.ctx.graph.read().unwrap();
            match graph.resolve_node_id(node_query, kind) {
                Ok(id) => id,
                Err(coderet_graph::graph::ResolutionError::Ambiguous(query, candidates)) => {
                    let mut out = format!("Ambiguous node '{}'. Did you mean one of these?\n", query);
                    for (i, c) in candidates.iter().enumerate() {
                        out.push_str(&format!("{}. {}\n", i + 1, c));
                    }
                    out.push_str("\nPlease call this tool again with the 'kind' argument set to 'file' or 'symbol' to disambiguate.");
                    return Ok(out);
                }
                Err(coderet_graph::graph::ResolutionError::NotFound(query)) => {
                    return Ok(format!("Node '{}' not found in the graph. Try searching for the symbol first using 'search_code' to find the correct name or ID.", query));
                }
                Err(e) => return Err(e.into()),
            }
        };

        // 2. Traverse the graph using the resolved ID
        let direction = match relation {
            "incoming" => GraphDirection::In,
            "both" => GraphDirection::Both,
            _ => GraphDirection::Out,
        };

        let result = (*self.inner).graph(&node_id, direction, max_hops)?;
        
        // Return JSON by default for the agent
        let json_output = serde_json::to_string_pretty(&result.subgraph)?;
        Ok(json_output)
    }
}

