use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct JavaSupport;

impl LanguageSupport for JavaSupport {
    fn language(&self) -> Language {
        Language::Java
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
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
        parser.set_language(&tree_sitter_java::LANGUAGE.into())?;
        Ok(parser)
    }
}
