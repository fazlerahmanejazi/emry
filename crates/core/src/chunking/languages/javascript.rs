use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct JavaScriptSupport;

impl LanguageSupport for JavaScriptSupport {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_declaration) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(class_declaration) @class".to_string(),
                priority: 5,
            },
            ChunkQuery {
                pattern: "(method_definition) @method".to_string(),
                priority: 10,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_javascript::LANGUAGE.into())?;
        Ok(parser)
    }
}
