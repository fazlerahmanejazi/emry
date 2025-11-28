use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct CSupport;

impl LanguageSupport for CSupport {
    fn language(&self) -> Language {
        Language::C
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_definition) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(struct_specifier) @struct".to_string(),
                priority: 5,
            },
            ChunkQuery {
                pattern: "(enum_specifier) @enum".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_c::LANGUAGE.into())?;
        Ok(parser)
    }
}
