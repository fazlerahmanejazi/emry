use crate::chunking::tokenizer::count_tokens;
use crate::models::Chunk;
use anyhow::Result;
use emry_config::{ChunkingConfig, SplitStrategy};
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::cl100k_base;

static TOKENIZER: Lazy<tiktoken_rs::CoreBPE> =
    Lazy::new(|| cl100k_base().expect("Failed to load tokenizer"));

/// Enforce token limits on chunks, splitting or truncating as needed
pub fn enforce_token_limits(chunks: Vec<Chunk>, config: &ChunkingConfig) -> Result<Vec<Chunk>> {
    let mut result = Vec::new();

    for chunk in chunks {
        let token_count = count_tokens(&chunk.content);

        if token_count <= config.max_tokens {
            result.push(chunk);
        } else {
            // Chunk is too large, split it
            let split_chunks = split_chunk(chunk, config)?;
            result.extend(split_chunks);
        }
    }

    Ok(result)
}

fn split_chunk(chunk: Chunk, config: &ChunkingConfig) -> Result<Vec<Chunk>> {
    match config.strategy {
        SplitStrategy::Truncate => Ok(vec![truncate_chunk(chunk, config.max_tokens)]),
        SplitStrategy::Split | SplitStrategy::Hierarchical => {
            // Use text-splitter for smart semantic chunking
            split_with_text_splitter(chunk, config)
        }
    }
}

fn truncate_chunk(mut chunk: Chunk, max_tokens: usize) -> Chunk {
    let tokens = count_tokens(&chunk.content);
    if tokens <= max_tokens {
        return chunk;
    }

    // Use text-splitter to get the first chunk within limits
    let chunk_config = ChunkConfig::new(max_tokens).with_sizer(TOKENIZER.clone());
    let splitter = TextSplitter::new(chunk_config);

    let chunks: Vec<&str> = splitter.chunks(&chunk.content).collect();

    if let Some(first_chunk) = chunks.first() {
        chunk.content = first_chunk.to_string();

        // Update content_hash
        let mut hasher = Sha256::new();
        hasher.update(chunk.file_path.to_string_lossy().as_bytes());
        hasher.update(chunk.content.as_bytes());
        chunk.content_hash = hex::encode(hasher.finalize());
        chunk.id = chunk.content_hash[..16].to_string();
    }

    chunk
}

fn split_with_text_splitter(chunk: Chunk, config: &ChunkingConfig) -> Result<Vec<Chunk>> {
    // Configure text-splitter with tokenizer and overlap
    let chunk_config = ChunkConfig::new(config.max_tokens)
        .with_sizer(TOKENIZER.clone())
        .with_overlap(config.overlap_tokens)
        .map_err(|e| anyhow::anyhow!("Invalid chunk config: {}", e))?;

    let splitter = TextSplitter::new(chunk_config);

    // Split text using semantic boundaries
    let text_chunks: Vec<&str> = splitter.chunks(&chunk.content).collect();

    // Convert text chunks back to our Chunk type
    let result: Vec<Chunk> = text_chunks
        .into_iter()
        .enumerate()
        .map(|(i, text)| create_sub_chunk_from_text(&chunk, text, i))
        .collect();

    Ok(result)
}

fn create_sub_chunk_from_text(original: &Chunk, text: &str, index: usize) -> Chunk {
    let mut hasher = Sha256::new();
    hasher.update(original.file_path.to_string_lossy().as_bytes());
    hasher.update(text.as_bytes());
    hasher.update(index.to_string().as_bytes());
    let hash = hex::encode(hasher.finalize());

    // Calculate line offsets (approximate based on position in original content)
    let text_position = original.content.find(text).unwrap_or(0);
    let lines_before: usize = original.content[..text_position].lines().count();
    let lines_in_chunk = text.lines().count().max(1);

    Chunk {
        id: hash[..16].to_string(),
        language: original.language.clone(),
        file_path: original.file_path.clone(),
        start_line: original.start_line + lines_before,
        end_line: original.start_line + lines_before + lines_in_chunk,
        start_byte: None,
        end_byte: None,
        node_type: if index == 0 {
            original.node_type.clone()
        } else {
            format!("{}_part{}", original.node_type, index + 1)
        },
        content_hash: hash,
        content: text.to_string(),
        embedding: None,
        parent_scope: original.parent_scope.clone(),
        scope_path: original.scope_path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Language;
    use std::path::PathBuf;

    fn create_test_chunk(content: String) -> Chunk {
        Chunk {
            id: "test".to_string(),
            language: Language::Python,
            file_path: PathBuf::from("test.py"),
            start_line: 1,
            end_line: 10,
            start_byte: None,
            end_byte: None,
            node_type: "function".to_string(),
            content_hash: "hash".to_string(),
            content,
            embedding: None,
            parent_scope: None,
            scope_path: Vec::new(),
        }
    }

    #[test]
    fn test_truncate_small_chunk() {
        let chunk = create_test_chunk("def foo(): pass".to_string());
        let config = ChunkingConfig {
            max_tokens: 100,
            ..Default::default()
        };

        let result = enforce_token_limits(vec![chunk.clone()], &config).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, chunk.content);
    }

    #[test]
    fn test_truncate_large_chunk() {
        let content = "def foo():\n".to_string() + &"    x = 1\n".repeat(100);
        let chunk = create_test_chunk(content);
        let config = ChunkingConfig {
            max_tokens: 50,
            strategy: SplitStrategy::Truncate,
            use_cast: false,
            max_chars: 2000,
            ..Default::default()
        };

        let result = enforce_token_limits(vec![chunk], &config).unwrap();
        assert_eq!(result.len(), 1);
        let tokens = count_tokens(&result[0].content);
        assert!(tokens <= 50);
    }

    #[test]
    fn test_split_large_chunk() {
        let content = "def foo():\n".to_string() + &"    x = 1\n".repeat(200);
        let chunk = create_test_chunk(content);
        let config = ChunkingConfig {
            max_tokens: 100,
            overlap_tokens: 10,
            strategy: SplitStrategy::Split,
            use_cast: false,
            max_chars: 2000,
            ..Default::default()
        };

        let result = enforce_token_limits(vec![chunk], &config).unwrap();
        assert!(result.len() > 1, "Should split into multiple chunks");

        // Verify all chunks are within token limit
        for chunk in &result {
            let tokens = count_tokens(&chunk.content);
            assert!(tokens <= 100, "Chunk has {} tokens, max is 100", tokens);
        }
    }

    #[test]
    fn test_semantic_splitting() {
        let content = "This is a sentence. This is another sentence. And one more.".repeat(20);
        let chunk = create_test_chunk(content);
        let config = ChunkingConfig {
            max_tokens: 50,
            overlap_tokens: 5,
            strategy: SplitStrategy::Split,
            use_cast: false,
            max_chars: 2000,
            ..Default::default()
        };

        let result = enforce_token_limits(vec![chunk], &config).unwrap();
        assert!(result.len() > 1);

        // text-splitter should split at sentence boundaries
        for chunk in &result {
            let tokens = count_tokens(&chunk.content);
            assert!(tokens <= 50);
        }
    }
}
