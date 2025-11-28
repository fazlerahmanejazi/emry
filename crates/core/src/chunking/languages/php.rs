use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct PhpSupport;

impl LanguageSupport for PhpSupport {
    fn language(&self) -> Language {
        Language::Php
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_definition) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(method_declaration) @method".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(class_declaration) @class".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into())?;
        Ok(parser)
    }
}
