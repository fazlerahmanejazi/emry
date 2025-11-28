use crate::models::{Language, Symbol};
use crate::tags_extractor::TagsExtractor;
use anyhow::Result;
use std::path::Path;

/// Extract symbols for a given language.
/// Uses stack-graphs for supported languages (Rust, Python, JavaScript, TypeScript, Java),
/// falling back to tree-sitter-tags if stack-graphs fails or for unsupported languages.
pub fn extract_symbols(content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {
    // Try stack-graphs for supported languages
    // Try stack-graphs for supported languages
    // Note: We currently disable per-file stack-graphs extraction here because it is inefficient
    // (builds a new graph per file) and we are building a global stack-graph later in the pipeline.
    // For simple symbol lists, tree-sitter-tags is sufficient and faster.
    /*
    if supports_stack_graphs(language) {
        if let Ok(symbols) = extract_symbols_stack_graphs(content, path, language) {
            if !symbols.is_empty() {
                return Ok(symbols);
            }
        }
        // Fall through to tags extractor if stack-graphs fails or returns nothing
    }
    */
    
    // Create extractor per-call since TagsConfiguration isn't Send
    // This is fine - extraction only happens during indexing, not on hot path
    let mut extractor = TagsExtractor::new()?;
    extractor.extract_symbols(content, path, language)
}


