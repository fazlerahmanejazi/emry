use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Language {
    Python,
    Java,
    TypeScript,
    JavaScript,
    Cpp,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "py" => Language::Python,
            "java" => Language::Java,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" => Language::JavaScript,
            "cpp" | "cxx" | "cc" | "hpp" | "hh" | "h" => Language::Cpp,
            _ => Language::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub language: Language,
    pub file_path: PathBuf,
    pub start_line: usize, // 1-indexed
    pub end_line: usize,   // 1-indexed
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
    pub node_type: String, // e.g., "function", "class"
    pub content_hash: String,
    pub content: String,
    pub embedding: Option<Vec<f32>>, // Optional for now, populated later
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub content_hash: String,
    pub last_modified: u64, // Timestamp
    pub chunk_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub version: String,
    pub files: Vec<FileMetadata>,
}
