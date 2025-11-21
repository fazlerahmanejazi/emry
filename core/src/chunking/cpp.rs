use super::Chunker;
use crate::models::{Chunk, Language};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct CppChunker;

impl CppChunker {
    pub fn new() -> Self {
        Self
    }
}

impl Chunker for CppChunker {
    fn chunk(&self, content: &str, file_path: &Path) -> Result<Vec<Chunk>> {
        let mut parser = Parser::new();
        let language = tree_sitter_cpp::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| anyhow!("Failed to set C++ language: {}", e))?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse C++ code"))?;

        let mut chunks = Vec::new();
        
        // Query to find classes, structs, functions, and namespaces
        let query_str = "
            (class_specifier) @class
            (struct_specifier) @struct
            (function_definition) @function
            (namespace_definition) @namespace
        ";
        
        let query = Query::new(&language.into(), query_str)
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
                    language: Language::Cpp,
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
        
        Ok(chunks)
    }
}
