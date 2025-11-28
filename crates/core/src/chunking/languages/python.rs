use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct PythonSupport;

impl LanguageSupport for PythonSupport {
    fn language(&self) -> Language {
        Language::Python
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_definition) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(class_definition) @class".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
        Ok(parser)
    }
}
