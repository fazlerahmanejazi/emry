use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::retriever::Retriever;
use crate::config::SearchMode;
use crate::config::Config;
use crate::structure::graph::CodeGraph;
use crate::paths::builder::{PathBuilder, PathBuilderConfig};
use crate::structure::graph::NodeId;
use std::collections::HashMap;
use globset::{Glob, GlobSetBuilder};
use crate::summaries::index::SummaryIndex;
use crate::summaries::index::SummaryLevel;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn call(&self, args: Value) -> Result<String>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }
    
    pub fn list_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

pub struct SearchTool {
    retriever: Arc<Retriever>,
}

impl SearchTool {
    pub fn new(retriever: Arc<Retriever>) -> Self {
        Self { retriever }
    }
}

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for code snippets using semantic and lexical search. Args: { \"query\": \"string\", \"mode\"?: \"lexical\"|\"semantic\"|\"hybrid\", \"top\"?: number, \"lang\"?: string, \"path\"?: glob }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or_default();
        if query.is_empty() {
            return Ok("Query cannot be empty".to_string());
        }

        let top = args["top"].as_u64().unwrap_or(5) as usize;
        let mode = match args["mode"].as_str().map(|s| s.to_lowercase()) {
            Some(m) if m == "lexical" => SearchMode::Lexical,
            Some(m) if m == "semantic" => SearchMode::Semantic,
            _ => SearchMode::Hybrid,
        };
        let lang_filter = args["lang"].as_str().map(|s| s.to_lowercase());
        let path_glob = args["path"].as_str();

        let (mut results, _summaries) = match self
            .retriever
            .search_with_summaries(query, mode.clone(), top, 0.1, 0.25)
            .await
        {
            Ok(r) => r,
            Err(_) => (self.retriever.search(query, mode, top).await?, Vec::new()),
        };

        if let Some(lang) = lang_filter {
            results.retain(|r| r.chunk.language.to_string().to_lowercase() == lang);
        }

        if let Some(glob_str) = path_glob {
            if let Ok(glob) = Glob::new(glob_str) {
                let mut builder = GlobSetBuilder::new();
                builder.add(glob);
                if let Ok(matcher) = builder.build() {
                    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    results.retain(|r| {
                        let rel = r
                            .chunk
                            .file_path
                            .strip_prefix(&root)
                            .unwrap_or(&r.chunk.file_path)
                            .to_string_lossy()
                            .to_string();
                        matcher.is_match(rel)
                    });
                }
            }
        }

        let mut output = String::new();
        for res in results {
            output.push_str(&format!(
                "File: {}:{}-{}\nScore: {:.2}\nContent:\n{}\n\n",
                res.chunk.file_path.display(),
                res.chunk.start_line,
                res.chunk.end_line,
                res.score,
                res.chunk.content
            ));
        }
        Ok(output)
    }
}

pub struct PathTool {
    graph: Arc<CodeGraph>,
}

impl PathTool {
    pub fn new(graph: Arc<CodeGraph>) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for PathTool {
    fn name(&self) -> &str {
        "find_path"
    }

    fn description(&self) -> &str {
        "Find a path between two code entities. Args: { \"start\": \"node_id\", \"end\": \"node_id\" }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let start_id = args["start"].as_str().unwrap_or_default();
        let end_id = args["end"].as_str().unwrap_or_default();

        if start_id.is_empty() || end_id.is_empty() {
            return Ok("Start and End IDs are required".to_string());
        }

        let builder = PathBuilder::new(&self.graph);
        let config = PathBuilderConfig::default();
        
        // Use bi-directional search for efficiency
        let paths = builder.find_paths_bidirectional(
            &NodeId(start_id.to_string()), 
            &NodeId(end_id.to_string()), 
            &config
        );
        
        let mut output = String::new();
        if paths.is_empty() {
            return Ok("No path found.".to_string());
        }

        output.push_str(&format!("Found {} paths:\n", paths.len()));
        for (i, path) in paths.iter().enumerate() {
            output.push_str(&format!("Path #{}:\n", i + 1));
            for (j, node) in path.nodes.iter().enumerate() {
                if j > 0 {
                    output.push_str(" -> ");
                }
                output.push_str(&node.node_id);
            }
            output.push_str("\n");
        }

        Ok(output)
    }
}

pub struct ReadCodeTool;

impl ReadCodeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadCodeTool {
    fn name(&self) -> &str {
        "read_code"
    }

    fn description(&self) -> &str {
        "Read the full content of a file. Args: { \"path\": \"file_path\" }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let path_str = args["path"].as_str().unwrap_or_default();
        if path_str.is_empty() {
            return Ok("Path argument is required".to_string());
        }

        let path = std::path::Path::new(path_str);
        if !path.exists() {
            return Ok(format!("File not found: {}", path_str));
        }

        match std::fs::read_to_string(path) {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!("Error reading file: {}", e)),
        }
    }
}

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern in the codebase. Args: { \"pattern\": \"regex_string\" }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or_default();
        if pattern.is_empty() {
            return Ok("Pattern argument is required".to_string());
        }

        let no_ignore = args["no_ignore"].as_bool().unwrap_or(false);
        let config = Config::load().unwrap_or_default();
        let root = std::env::current_dir()?;
        match crate::index::regex::RegexSearcher::search_with_ignore(
            &root,
            pattern,
            &config.index,
            !no_ignore,
        ) {
            Ok(matches) => {
                if matches.is_empty() {
                    Ok("No matches found.".to_string())
                } else {
                    let mut output = String::new();
                    output.push_str(&format!("Found {} matches:\n", matches.len()));
                    for (path, line, content) in matches.iter().take(20) { // Limit to 20
                        output.push_str(&format!("{}:{}: {}\n", path.display(), line, content.trim()));
                    }
                    if matches.len() > 20 {
                        output.push_str("... (more matches truncated)\n");
                    }
                    Ok(output)
                }
            }
            Err(e) => Ok(format!("Error executing grep: {}", e)),
        }
    }
}

use crate::structure::index::SymbolIndex;

pub struct SymbolTool {
    index: Arc<SymbolIndex>,
}

impl SymbolTool {
    pub fn new(index: Arc<SymbolIndex>) -> Self {
        Self { index }
    }
}

#[async_trait]
impl Tool for SymbolTool {
    fn name(&self) -> &str {
        "lookup_symbol"
    }

    fn description(&self) -> &str {
        "Find the definition of a symbol (class, function, etc.). Args: { \"name\": \"symbol_name\" }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let name = args["name"].as_str().unwrap_or_default();
        if name.is_empty() {
            return Ok("Name argument is required".to_string());
        }

        let matches = self.index.search(name);
        if matches.is_empty() {
            return Ok(format!("Symbol '{}' not found.", name));
        }

        let mut output = String::new();
        for sym in matches {
            output.push_str(&format!(
                "Symbol: {}\nKind: {:?}\nFile: {}:{}-{}\n\n",
                sym.name, sym.kind, sym.file_path.display(), sym.start_line, sym.end_line
            ));
        }
        Ok(output)
    }
}

pub struct ReferencesTool;

impl ReferencesTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReferencesTool {
    fn name(&self) -> &str {
        "find_references"
    }

    fn description(&self) -> &str {
        "Find all references/usages of a symbol in the codebase. Args: { \"symbol\": \"symbol_name\" }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or_default();
        if symbol.is_empty() {
            return Ok("Symbol argument is required".to_string());
        }

        let root = std::env::current_dir()?;
        let no_ignore = args["no_ignore"].as_bool().unwrap_or(false);
        let config = Config::load().unwrap_or_default();
        // Use regex search to find references (simple word boundary match)
        let pattern = format!(r"\b{}\b", regex::escape(symbol));
        
        match crate::index::regex::RegexSearcher::search_with_ignore(
            &root,
            &pattern,
            &config.index,
            !no_ignore,
        ) {
            Ok(matches) => {
                if matches.is_empty() {
                    Ok(format!("No references found for '{}'.", symbol))
                } else {
                    let mut output = String::new();
                    output.push_str(&format!("Found {} references to '{}':\n", matches.len(), symbol));
                    for (path, line, content) in matches.iter().take(15) {
                        output.push_str(&format!("{}:{}: {}\n", path.display(), line, content.trim()));
                    }
                    if matches.len() > 15 {
                        output.push_str("... (more references truncated)\n");
                    }
                    Ok(output)
                }
            }
            Err(e) => Ok(format!("Error finding references: {}", e)),
        }
    }
}

pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List files and directories in a given path. Args: { \"path\": \"directory_path\" }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let path_str = args["path"].as_str().unwrap_or(".");
        let path = std::path::Path::new(path_str);
        
        if !path.exists() {
            return Ok(format!("Path does not exist: {}", path_str));
        }

        if !path.is_dir() {
            return Ok(format!("Path is not a directory: {}", path_str));
        }

        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut output = String::new();
                output.push_str(&format!("Contents of {}:\n", path_str));
                
                let mut dirs = Vec::new();
                let mut files = Vec::new();
                
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_dir() {
                            dirs.push(format!("{}/", name));
                        } else {
                            files.push(name);
                        }
                    }
                }
                
                dirs.sort();
                files.sort();
                
                for dir in dirs {
                    output.push_str(&format!("  {}\n", dir));
                }
                for file in files {
                    output.push_str(&format!("  {}\n", file));
                }
                
                Ok(output)
            }
            Err(e) => Ok(format!("Error reading directory: {}", e)),
        }
    }
}

pub struct SummaryTool {
    index: Arc<SummaryIndex>,
}

impl SummaryTool {
    pub fn new(index: Arc<SummaryIndex>) -> Self {
        Self { index }
    }
}

#[async_trait]
impl Tool for SummaryTool {
    fn name(&self) -> &str {
        "summaries"
    }

    fn description(&self) -> &str {
        "Search stored summaries. Args: { \"query\": \"string\", \"top\"?: number, \"level\"?: \"function\"|\"class\"|\"file\"|\"module\"|\"repo\", \"lang\"?: string }"
    }

    async fn call(&self, args: Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or_default().to_lowercase();
        if query.is_empty() {
            return Ok("Query cannot be empty".to_string());
        }
        let top = args["top"].as_u64().unwrap_or(5) as usize;
        let lang_filter = args["lang"].as_str().map(|s| s.to_lowercase());
        let level_filter = args["level"]
            .as_str()
            .and_then(|s| match s.to_lowercase().as_str() {
                "function" => Some(SummaryLevel::Function),
                "class" => Some(SummaryLevel::Class),
                "file" => Some(SummaryLevel::File),
                "module" => Some(SummaryLevel::Module),
                "repo" => Some(SummaryLevel::Repo),
                _ => None,
            });

        let mut scored: Vec<_> = self
            .index
            .summaries
            .values()
            .filter(|s| {
                let level_ok = level_filter.as_ref().map_or(true, |lf| &s.level == lf);
                let lang_ok = lang_filter.as_ref().map_or(true, |lf| {
                    s.language
                        .as_ref()
                        .map(|l| l.to_lowercase() == *lf)
                        .unwrap_or(false)
                });
                level_ok && lang_ok
            })
            .map(|s| {
                let hay = format!("{} {}", s.text.to_lowercase(), s.id.to_lowercase());
                let score = if hay.contains(&query) {
                    // Simple lexical score: shorter distance to match gets higher score
                    let idx = hay.find(&query).unwrap_or(usize::MAX);
                    (query.len() as i32).saturating_sub(idx as i32)
                } else {
                    -1
                };
                (score, s)
            })
            .filter(|(score, _)| *score >= 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.truncate(top);

        if scored.is_empty() {
            return Ok("No summaries matched.".to_string());
        }

        let mut output = String::new();
        for (_score, s) in scored {
            let location = match (&s.file_path, s.start_line, s.end_line) {
                (Some(p), Some(start), Some(end)) => format!("{}:{}-{}", p.display(), start, end),
                _ => s.target_id.clone(),
            };
            output.push_str(&format!(
                "Summary: {}\nLevel: {:?}\nTarget: {}\nLocation: {}\nText: {}\n\n",
                s.id, s.level, s.target_id, location, s.text
            ));
        }
        Ok(output)
    }
}
