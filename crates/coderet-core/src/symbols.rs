use crate::models::{Language, Symbol};
use crate::tags_extractor::TagsExtractor;
use crate::stack_graphs_symbols::{extract_symbols_stack_graphs, supports_stack_graphs};
use anyhow::Result;
use std::path::Path;

/// Extract symbols for a given language.
/// Uses stack-graphs for supported languages (Rust, Python, JavaScript, TypeScript, Java),
/// falling back to tree-sitter-tags if stack-graphs fails or for unsupported languages.
pub fn extract_symbols(content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {
    // Try stack-graphs for supported languages
    if supports_stack_graphs(language) {
        if let Ok(symbols) = extract_symbols_stack_graphs(content, path, language) {
            if !symbols.is_empty() {
                return Ok(symbols);
            }
        }
        // Fall through to tags extractor if stack-graphs fails or returns nothing
    }
    
    // Create extractor per-call since TagsConfiguration isn't Send
    // This is fine - extraction only happens during indexing, not on hot path
    let mut extractor = TagsExtractor::new()?;
    extractor.extract_symbols(content, path, language)
}


