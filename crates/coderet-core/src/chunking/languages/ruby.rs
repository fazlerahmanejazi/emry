use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct RubySupport;

impl LanguageSupport for RubySupport {
    fn language(&self) -> Language {
        Language::Ruby
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(method) @method".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(module) @module".to_string(),
                priority: 5,
            },
            ChunkQuery {
                pattern: "(class) @class".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_ruby::LANGUAGE.into())?;
        Ok(parser)
    }
}
