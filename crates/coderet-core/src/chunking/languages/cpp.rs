use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct CppSupport;

impl LanguageSupport for CppSupport {
    fn language(&self) -> Language {
        Language::Cpp
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_definition) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(class_specifier) @class".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_cpp::LANGUAGE.into())?;
        Ok(parser)
    }
}
