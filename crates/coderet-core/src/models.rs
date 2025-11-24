use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Rust,
    Go,
    Java,
    Cpp,
    C,
    Ruby,
    Php,
    CSharp,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "py" => Language::Python,
            "js" | "jsx" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "rs" => Language::Rust,
            "go" => Language::Go,
            "java" => Language::Java,
            "cpp" | "cc" | "cxx" | "h" | "hpp" => Language::Cpp,
            "c" => Language::C,
            "rb" => Language::Ruby,
            "php" => Language::Php,
            "cs" => Language::CSharp,
            _ => Language::Unknown,
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "python" => Language::Python,
            "javascript" => Language::JavaScript,
            "typescript" => Language::TypeScript,
            "rust" => Language::Rust,
            "go" => Language::Go,
            "java" => Language::Java,
            "cpp" => Language::Cpp,
            "c" => Language::C,
            "ruby" => Language::Ruby,
            "php" => Language::Php,
            "csharp" => Language::CSharp,
            _ => Language::Unknown,
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A chunk of code to be indexed.
/// In the new design, this is a Data Transfer Object (DTO).
/// Storage will separate metadata from content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub language: Language,
    pub file_path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
    pub node_type: String,
    pub content_hash: String,
    pub content: String, // Kept for now as DTO, but storage will strip it
    pub embedding: Option<Vec<f32>>,
    pub parent_scope: Option<String>,
    pub scope_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: String, // e.g., "function", "class"
    pub file_path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub fqn: String, // Fully Qualified Name
    pub language: Language,
    pub doc_comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub level: coderet_config::SummaryLevel,
    pub target_id: String,
    pub canonical_target_id: Option<String>,
    pub text: String,
    pub file_path: Option<PathBuf>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub name: Option<String>,
    pub language: Option<String>,
    /// Logical module/namespace for this summary (e.g., top-level directory name).
    #[serde(default)]
    pub module: Option<String>,
    pub model: Option<String>,
    pub prompt_version: Option<String>,
    pub generated_at: Option<u64>,
    pub source_hash: Option<String>,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct ScoredChunk {
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub graph_boost: Option<f32>,
    pub graph_distance: Option<usize>,
    pub graph_path: Option<Vec<String>>,
    pub symbol_boost: Option<f32>,
    pub summary_score: Option<f32>,
    pub chunk: crate::models::Chunk,
}

#[derive(Debug, Clone)]
pub struct ContextualResult {
    pub chunks: Vec<ScoredChunk>,
    pub paths: Vec<paths::Path>,
    pub summaries: Vec<Summary>,
}

// Re-export Path for convenience if needed, but it's in coderet-graph
pub mod paths {
    // This is a placeholder; actual Path struct is in coderet-graph.
    // We use a simple DTO here if we want to decouple, or reference coderet-graph.
    // For now, let's assume we use a simplified representation or generic JSON.
    // Actually, to avoid circular deps, we might need to define a Path DTO here.
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Path {
        pub nodes: Vec<String>, // Just labels or IDs
        pub edges: Vec<String>,
        pub score: f32,
    }
}
