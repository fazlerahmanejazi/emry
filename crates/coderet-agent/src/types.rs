use coderet_core::models::{Chunk, Language, Symbol};
use coderet_graph::graph::GraphNode;
use coderet_index::summaries::SummaryRecord;

#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub graph_path: Option<Vec<String>>,
    pub chunk: Chunk,
}

#[derive(Debug, Clone)]
pub struct SummaryHit {
    pub score: f32,
    pub summary: SummaryRecord,
}

#[derive(Debug, Clone)]
pub struct SymbolHit {
    pub name: String,
    pub file_path: String,
    pub language: Language,
    pub start_line: usize,
    pub end_line: usize,
    pub symbol: Symbol,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct GraphSubgraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}
