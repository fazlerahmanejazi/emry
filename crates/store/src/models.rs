use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileRecord {
    pub id: Option<Thing>,
    pub path: String,
    pub language: String,
    pub content: String,
    pub hash: String,
    pub last_modified: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChunkRecord {
    pub id: Option<Thing>,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub file: Thing,
    pub start_line: usize,
    pub end_line: usize,
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SymbolRecord {
    pub id: Option<Thing>,
    pub name: String,
    pub kind: String,
    pub file: Thing,
    pub start_line: usize,
    pub end_line: usize,
}

// Edge Relations
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct DefinesRelation {
    pub r#in: Thing,
    pub out: Thing,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SurrealGraphNode {
    pub id: Thing,
    pub label: String,
    pub kind: String,
    pub file_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SurrealGraphEdge {
    pub source: Thing,
    pub target: Thing,
    pub relation: String,
    pub target_node: Option<SurrealGraphNode>, // Optional: if we fetch target details
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommitLogRecord {
    pub id: Option<Thing>,
    pub commit_id: String,
    pub timestamp: u64,
    pub note: String,
}
