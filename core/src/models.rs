use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Language {
    Python,
    Java,
    TypeScript,
    JavaScript,
    Cpp,
    Go,
    Rust,
    Ruby,
    Php,
    CSharp,
    C,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "py" => Language::Python,
            "java" => Language::Java,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" => Language::JavaScript,
            "cpp" | "cxx" | "cc" | "hpp" | "hh" => Language::Cpp,
            "go" => Language::Go,
            "rs" => Language::Rust,
            "rb" => Language::Ruby,
            "php" => Language::Php,
            "cs" => Language::CSharp,
            "c" | "h" => Language::C,
            _ => Language::Unknown,
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "python" | "py" => Language::Python,
            "java" => Language::Java,
            "typescript" | "ts" | "tsx" => Language::TypeScript,
            "javascript" | "js" | "jsx" => Language::JavaScript,
            "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" => Language::Cpp,
            "go" | "golang" => Language::Go,
            "rust" | "rs" => Language::Rust,
            "ruby" | "rb" => Language::Ruby,
            "php" => Language::Php,
            "csharp" | "c#" | "cs" => Language::CSharp,
            "c" => Language::C,
            _ => Language::Unknown,
        }
    }
}

impl std::str::FromStr for Language {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Language::from_name(s))
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Language::Python => "python",
            Language::Java => "java",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Cpp => "cpp",
            Language::Go => "go",
            Language::Rust => "rust",
            Language::Ruby => "ruby",
            Language::Php => "php",
            Language::CSharp => "csharp",
            Language::C => "c",
            Language::Unknown => "unknown",
        };
        write!(f, "{}", name)
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
    #[serde(default)]
    pub parent_scope: Option<String>, // e.g., "class Foo"
    #[serde(default)]
    pub scope_path: Vec<String>, // ancestors, outermost -> innermost
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

impl Default for IndexMetadata {
    fn default() -> Self {
        Self {
            version: "1".to_string(),
            files: Vec::new(),
        }
    }
}
