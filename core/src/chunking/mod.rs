use crate::models::Chunk;
use anyhow::Result;

pub trait Chunker {
    fn chunk(&self, content: &str, file_path: &std::path::Path) -> Result<Vec<Chunk>>;
}

pub mod config;
pub mod generic;
pub mod splitter;
pub mod tokenizer;

pub use config::{ChunkingConfig, SplitStrategy};
pub use generic::GenericChunker;
