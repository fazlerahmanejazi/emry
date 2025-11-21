use super::Chunker;
use crate::models::{Chunk, Language};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct TypeScriptChunker;

impl TypeScriptChunker {
    pub fn new() -> Self {
        Self
    }
}

impl Chunker for TypeScriptChunker {
    fn chunk(&self, content: &str, file_path: &Path) -> Result<Vec<Chunk>> {
        let mut parser = Parser::new();
        
        // Determine if TS or TSX based on extension
        let is_tsx = file_path.extension().map_or(false, |ext| ext == "tsx" || ext == "jsx");
        let language = if is_tsx {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };

        parser
            .set_language(&language.into())
            .map_err(|e| anyhow!("Failed to set TypeScript language: {}", e))?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse TypeScript code"))?;

        let mut chunks = Vec::new();
        
        // Query for functions, methods, classes, interfaces
        let query_str = "
            (function_declaration) @function
            (method_definition) @method
            (class_declaration) @class
            (interface_declaration) @interface
            (arrow_function) @arrow
        ";
        
        let query = Query::new(&language.into(), query_str)
            .map_err(|e| anyhow!("Failed to create query: {}", e))?;
            
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        for m in matches {
            for capture in m.captures {
                let node = capture.node;
                
                // Filter out small arrow functions if needed, or keep all.
                // For now, keep all.

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
                    language: if is_tsx { Language::TypeScript } else { Language::TypeScript }, // Simplify to TS for now or add TSX to enum
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
