use crate::models::Chunk;
use anyhow::Result;

pub trait Chunker {
    fn chunk(&self, content: &str, file_path: &std::path::Path) -> Result<Vec<Chunk>>;
}

pub mod generic;
pub mod config;
pub mod tokenizer;
pub mod splitter;

pub use generic::GenericChunker;
pub use config::{ChunkingConfig, SplitStrategy};
