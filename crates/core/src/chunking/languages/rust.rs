use super::{ChunkQuery, LanguageSupport};
use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub struct RustSupport;

impl LanguageSupport for RustSupport {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn get_queries(&self) -> Vec<ChunkQuery> {
        vec![
            ChunkQuery {
                pattern: "(function_item) @function".to_string(),
                priority: 10,
            },
            ChunkQuery {
                pattern: "(impl_item) @impl".to_string(),
                priority: 8,
            },
            ChunkQuery {
                pattern: "(struct_item) @struct".to_string(),
                priority: 5,
            },
            ChunkQuery {
                pattern: "(enum_item) @enum".to_string(),
                priority: 5,
            },
            ChunkQuery {
                pattern: "(trait_item) @trait".to_string(),
                priority: 5,
            },
        ]
    }

    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
        Ok(parser)
    }
}
