use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct GoSupport;

impl LanguageSupport for GoSupport {
    fn language(&self) -> Language {
        Language::Go
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_declaration) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(method_declaration) @method".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(type_declaration) @type".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::LANGUAGE.into())?;
        Ok(parser)
    }
}
