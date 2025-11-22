use super::Chunker;
use super::config::ChunkingConfig;
use super::splitter::enforce_token_limits;
use crate::models::{Chunk, Language};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct GenericChunker {
    language: Language,
    queries: Vec<ChunkQuery>,
    config: ChunkingConfig,
}

#[derive(Debug, Clone)]
pub struct ChunkQuery {
    pub pattern: String,
    pub priority: u8,
}

impl GenericChunker {
    pub fn new(language: Language) -> Self {
        Self::with_config(language, ChunkingConfig::default())
    }
    
    pub fn with_config(language: Language, config: ChunkingConfig) -> Self {
        let queries = Self::get_queries_for_language(&language);
        Self { language, queries, config }
    }
    
    fn get_queries_for_language(lang: &Language) -> Vec<ChunkQuery> {
        match lang {
            Language::Python => vec![
                ChunkQuery { pattern: "(function_definition) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(class_definition) @class".to_string(), priority: 5 },
            ],
            Language::Go => vec![
                ChunkQuery { pattern: "(function_declaration) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(method_declaration) @method".to_string(), priority: 10 },
                ChunkQuery { pattern: "(type_declaration) @type".to_string(), priority: 5 },
            ],
            Language::Rust => vec![
                ChunkQuery { pattern: "(function_item) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(impl_item) @impl".to_string(), priority: 8 },
                ChunkQuery { pattern: "(struct_item) @struct".to_string(), priority: 5 },
                ChunkQuery { pattern: "(enum_item) @enum".to_string(), priority: 5 },
                ChunkQuery { pattern: "(trait_item) @trait".to_string(), priority: 5 },
            ],
            Language::Java => vec![
                ChunkQuery { pattern: "(method_declaration) @method".to_string(), priority: 10 },
                ChunkQuery { pattern: "(class_declaration) @class".to_string(), priority: 5 },
            ],
            Language::TypeScript | Language::JavaScript => vec![
                ChunkQuery { pattern: "(function_declaration) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(class_declaration) @class".to_string(), priority: 5 },
                ChunkQuery { pattern: "(method_definition) @method".to_string(), priority: 10 },
            ],
            Language::Cpp => vec![
                ChunkQuery { pattern: "(function_definition) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(class_specifier) @class".to_string(), priority: 5 },
            ],
            Language::Ruby => vec![
                ChunkQuery { pattern: "(method) @method".to_string(), priority: 10 },
                ChunkQuery { pattern: "(module) @module".to_string(), priority: 5 },
                ChunkQuery { pattern: "(class) @class".to_string(), priority: 5 },
            ],
            Language::Php => vec![
                ChunkQuery { pattern: "(function_definition) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(method_declaration) @method".to_string(), priority: 10 },
                ChunkQuery { pattern: "(class_declaration) @class".to_string(), priority: 5 },
            ],
            Language::CSharp => vec![
                ChunkQuery { pattern: "(method_declaration) @method".to_string(), priority: 10 },
                ChunkQuery { pattern: "(class_declaration) @class".to_string(), priority: 5 },
                ChunkQuery { pattern: "(interface_declaration) @interface".to_string(), priority: 5 },
                ChunkQuery { pattern: "(struct_declaration) @struct".to_string(), priority: 5 },
            ],
            Language::C => vec![
                ChunkQuery { pattern: "(function_definition) @function".to_string(), priority: 10 },
                ChunkQuery { pattern: "(struct_specifier) @struct".to_string(), priority: 5 },
                ChunkQuery { pattern: "(enum_specifier) @enum".to_string(), priority: 5 },
            ],
            Language::Unknown => vec![],
        }
    }
    
    fn create_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        let lang = match self.language {
            Language::Python => tree_sitter_python::LANGUAGE,
            Language::Java => tree_sitter_java::LANGUAGE,
            Language::TypeScript | Language::JavaScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            Language::Cpp => tree_sitter_cpp::LANGUAGE,
            Language::Go => tree_sitter_go::LANGUAGE,
            Language::Rust => tree_sitter_rust::LANGUAGE,
            Language::Ruby => tree_sitter_ruby::LANGUAGE,
            Language::Php => tree_sitter_php::LANGUAGE_PHP,
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE,
            Language::C => tree_sitter_c::LANGUAGE,
            Language::Unknown => return Err(anyhow!("Unknown language")),
        };
        parser.set_language(&lang.into())?;
        Ok(parser)
    }
}

impl Chunker for GenericChunker {
    fn chunk(&self, content: &str, file_path: &Path) -> Result<Vec<Chunk>> {
        let mut parser = self.create_parser()?;
        
        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse {:?} code", self.language))?;

        let mut chunks = Vec::new();
        
        // Process each query
        for query_def in &self.queries {
            let query = Query::new(&parser.language().unwrap(), &query_def.pattern)
                .map_err(|e| anyhow!("Failed to create query: {}", e))?;
                
            let mut cursor = QueryCursor::new();
            let matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

            for m in matches {
                for capture in m.captures {
                    let node = capture.node;
                    let start_pos = node.start_position();
                    let end_pos = node.end_position();
                    
                    let start_line = start_pos.row + 1;
                    let end_line = end_pos.row + 1;
                    
                    let chunk_content = &content[node.start_byte()..node.end_byte()];
                    
                    let mut hasher = Sha256::new();
                    hasher.update(file_path.to_string_lossy().as_bytes());
                    hasher.update(chunk_content.as_bytes());
                    let hash = hex::encode(hasher.finalize());
                    let id = hash[..16].to_string();

                    let node_type = node.kind().to_string();

                    chunks.push(Chunk {
                        id,
                        language: self.language.clone(),
                        file_path: file_path.to_path_buf(),
                        start_line,
                        end_line,
                        start_byte: Some(node.start_byte()),
                        end_byte: Some(node.end_byte()),
                        node_type,
                        content_hash: hash,
                        content: chunk_content.to_string(),
                        embedding: None,
                    });
                }
            }
        }
        
        // Enforce token limits
        enforce_token_limits(chunks, &self.config)
    }
}
