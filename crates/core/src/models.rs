use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    pub content: String,
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
    pub parent_scope: Option<String>,
}



#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredChunk {
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub graph_boost: Option<f32>,
    pub graph_distance: Option<usize>,
    pub graph_path: Option<Vec<String>>,
    pub symbol_boost: Option<f32>,
    pub chunk: crate::models::Chunk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualResult {
    pub chunks: Vec<ScoredChunk>,
    pub paths: Vec<paths::Path>,
}

pub mod paths {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Path {
        pub nodes: Vec<String>, // Just labels or IDs
        pub edges: Vec<String>,
        pub score: f32,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextGraph {
    pub anchors: Vec<ScoredChunk>,
    pub related_files: Vec<File>,        // Parent files
    pub related_symbols: Vec<Symbol>,    // Callers/Callees
    pub edges: Vec<(String, String, String)>, // (from, to, relation)
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct File {
    pub id: String,
    pub path: String,
    pub language: Language,
    pub content: String,
}

pub struct ExpandedQuery {
    pub original: String,
    pub keywords: Vec<String>,
    pub intent: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolGroup {
    pub symbol: Symbol,
    pub anchors: Vec<ScoredChunk>,
    pub calls: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedContext {
    pub groups: Vec<SymbolGroup>,
    pub unassigned: Vec<ScoredChunk>,
}

impl ContextGraph {
    pub fn group_by_symbol(&self) -> GroupedContext {
        let mut symbol_groups: std::collections::HashMap<String, Vec<ScoredChunk>> = std::collections::HashMap::new();
        let mut unassigned: Vec<ScoredChunk> = Vec::new();

        for anchor in &self.anchors {
            // Find container symbol
            let container = self.edges.iter()
                .find(|(_, to, rel)| to == &anchor.chunk.id && rel == "contains")
                .and_then(|(from, _, _)| self.related_symbols.iter().find(|s| &s.id == from));
            
            if let Some(sym) = container {
                symbol_groups.entry(sym.id.clone()).or_default().push(anchor.clone());
            } else {
                unassigned.push(anchor.clone());
            }
        }

        let mut groups = Vec::new();
        for (sym_id, anchors) in symbol_groups {
            if let Some(sym) = self.related_symbols.iter().find(|s| s.id == sym_id) {
                // Find calls
                let calls: Vec<Symbol> = self.edges.iter()
                    .filter(|(from, _, rel)| from == &sym.id && rel == "calls")
                    .filter_map(|(_, to, _)| self.related_symbols.iter().find(|s| &s.id == to))
                    .cloned()
                    .collect();
                
                // Deduplicate calls
                let mut unique_calls = calls;
                unique_calls.sort_by(|a, b| a.name.cmp(&b.name));
                unique_calls.dedup_by(|a, b| a.name == b.name);

                groups.push(SymbolGroup {
                    symbol: sym.clone(),
                    anchors,
                    calls: unique_calls,
                });
            }
        }

        GroupedContext {
            groups,
            unassigned,
        }
    }
}

impl ScoredChunk {
    pub fn concatenate_chunks(chunks: &[ScoredChunk]) -> String {
        let mut sorted: Vec<&ScoredChunk> = chunks.iter().collect();
        sorted.sort_by_key(|c| c.chunk.start_line);

        let mut out = String::new();
        let mut last_end_line = 0;

        for (i, chunk) in sorted.iter().enumerate() {
            if i > 0 {
                let gap = chunk.chunk.start_line.saturating_sub(last_end_line + 1);
                if gap > 1 {
                    out.push_str(&format!("\n// ... (gap of {} lines) ...\n", gap));
                } else {
                    // Ensure newline between chunks if not present
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
            }
            out.push_str(&chunk.chunk.content);
            last_end_line = chunk.chunk.end_line;
        }
        out
    }
}
