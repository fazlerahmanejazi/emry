pub mod builder;
pub mod scorer;
pub mod selector;

use crate::structure::graph::{EdgeType, NodeType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathNode {
    pub node_id: String,
    pub kind: NodeType,
    pub name: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathEdge {
    pub from_node: String,
    pub to_node: String,
    pub kind: EdgeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Path {
    pub id: String,
    pub nodes: Vec<PathNode>,
    pub edges: Vec<PathEdge>,
    pub score: f32,
}

impl Path {
    pub fn new(nodes: Vec<PathNode>, edges: Vec<PathEdge>) -> Self {
        // Simple ID generation
        let id = format!(
            "path_{}_{}",
            nodes.first().map(|n| n.node_id.as_str()).unwrap_or(""),
            nodes.len()
        );
        Self {
            id,
            nodes,
            edges,
            score: 0.0,
        }
    }
}
