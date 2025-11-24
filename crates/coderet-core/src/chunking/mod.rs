pub mod generic;
pub mod splitter;
pub mod tokenizer;

pub use coderet_config::{ChunkingConfig, SplitStrategy};
pub use generic::GenericChunker;
pub use splitter::enforce_token_limits;

use crate::models::Chunk;
use anyhow::Result;
use std::path::Path;

pub trait Chunker {
    fn chunk(&self, content: &str, file_path: &Path) -> Result<Vec<Chunk>>;
}
