pub mod builder;
pub mod scorer;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathNode {
    pub node_id: String, // Graph Node ID
    pub kind: String,    // Function, Class, File, etc.
    pub name: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathEdge {
    pub from_node: String,
    pub to_node: String,
    pub kind: String, // Calls, DefinedIn, etc.
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
        // Simple ID generation (hash or uuid would be better in prod)
        let id = format!("path_{}", nodes.len()); 
        Self {
            id,
            nodes,
            edges,
            score: 0.0,
        }
    }
}
