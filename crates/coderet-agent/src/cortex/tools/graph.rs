use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use coderet_tools::graph::GraphTool as InnerGraphTool;
use coderet_tools::graph::GraphDirection;
use coderet_tools::GraphToolTrait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct InspectGraphTool {
    inner: Arc<InnerGraphTool>,
}

impl InspectGraphTool {
    pub fn new(inner: Arc<InnerGraphTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for InspectGraphTool {
    fn name(&self) -> &str {
        "inspect_graph"
    }

    fn description(&self) -> &str {
        "Explore the code graph to understand relationships (calls, imports, definitions). You MUST provide a valid Node ID (e.g., 'crates/core/lib.rs:10-20'), NOT a keyword."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "The Node ID to start from (must be a valid ID, not a search term)"
                },
                "relation": {
                    "type": "string",
                    "enum": ["incoming", "outgoing", "both"],
                    "description": "Direction of traversal",
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
        let node = args["node"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'node' argument"))?;
        let relation = args["relation"].as_str().unwrap_or("outgoing");
        let max_hops = args["max_hops"].as_u64().unwrap_or(1) as usize;

        let direction = match relation {
            "incoming" => GraphDirection::In,
            _ => GraphDirection::Out,
        };

        let result = (*self.inner).graph(node, direction, max_hops)?;
        
        let mut out = String::new();
        out.push_str("Nodes:\n");
        for n in &result.subgraph.nodes {
            out.push_str(&format!(" - {} ({})\n", n.id, n.label));
        }
        out.push_str("Edges:\n");
        for e in &result.subgraph.edges {
            out.push_str(&format!(" - {} -[{}]-> {}\n", e.source, e.kind, e.target));
        }
        
        Ok(out)
    }
}

pub struct ResolveEntityTool {
    ctx: std::sync::Arc<coderet_context::RepoContext>,
    search: std::sync::Arc<coderet_tools::search::Search>,
}

impl ResolveEntityTool {
    pub fn new(
        ctx: std::sync::Arc<coderet_context::RepoContext>,
        search: std::sync::Arc<coderet_tools::search::Search>,
    ) -> Self {
        Self { ctx, search }
    }
}

#[async_trait]
impl Tool for ResolveEntityTool {
    fn name(&self) -> &str {
        "resolve_entity"
    }

    fn description(&self) -> &str {
        "Resolve a name or keyword to a concrete Node ID in the graph. Use this BEFORE calling inspect_graph."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The name or keyword to resolve (e.g., 'User', 'auth_flow')"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;
            
        // Scope the lock so it's dropped before await
        let nodes = {
            let graph = self.ctx.graph.read().unwrap();
            graph.nodes_matching_label(query)?
        };
        
        let mut out = String::new();

        if !nodes.is_empty() {
            out.push_str(&format!("Found {} exact matches for '{}':\n", nodes.len(), query));
            for node in nodes {
                out.push_str(&format!(
                    "- ID: {}\n  Kind: {}\n  File: {}\n  Label: {}\n\n",
                    node.id, node.kind, node.file_path, node.label
                ));
            }
        } else {
            out.push_str(&format!("No exact matches for '{}'. Searching for relevant symbols...\n", query));
            
            // Fallback: Use Semantic/Lexical Search
            let results = self.search.search(query, 5).await?;
            
            if results.symbols.is_empty() {
                 return Ok(format!("No entities found matching '{}' (checked exact graph match and symbol search). Try a different query.", query));
            }

            out.push_str(&format!("Found {} relevant symbols:\n", results.symbols.len()));
            for sym in results.symbols {
                 out.push_str(&format!(
                    "- ID: {}\n  Kind: {}\n  File: {}\n  Label: {}\n  Score: {:.2}\n\n",
                    sym.symbol.id, sym.symbol.kind, sym.file_path, sym.name, 0.0 // Score not easily available in SymbolHit, but that's fine
                ));
            }
        }
        
        Ok(out)
    }
}
