use crate::models::{Language, Symbol};
use crate::tags_extractor::TagsExtractor;
use anyhow::Result;
use std::path::Path;

/// Extract symbols for a given language.
pub fn extract_symbols(content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {

    
    // Create extractor per-call since TagsConfiguration isn't Send
    let mut extractor = TagsExtractor::new()?;
    extractor.extract_symbols(content, path, language)
}


