use crate::models::{Language, Symbol};
use crate::tags_extractor::TagsExtractor;
use anyhow::Result;
use std::path::Path;

pub fn extract_symbols(content: &str, path: &Path, language: &Language) -> Result<Vec<Symbol>> {
    let mut extractor = TagsExtractor::new()?;
    extractor.extract_symbols(content, path, language)
}

pub fn generate_outline(content: &str, path: &Path, language: &Language) -> Result<String> {
    let mut extractor = TagsExtractor::new()?;
    extractor.generate_outline(content, path, language)
}

pub fn extract_code_item(content: &str, path: &Path, language: &Language, node_path: &str) -> Result<Option<String>> {
    let mut extractor = TagsExtractor::new()?;
    extractor.extract_code_item(content, path, language, node_path)
}


