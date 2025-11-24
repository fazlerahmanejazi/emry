use crate::models::Chunk;
use anyhow::Result;

use async_trait::async_trait;

pub trait Indexer {
    fn add_chunk(&mut self, chunk: Chunk) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
}

pub trait Retriever {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<(f32, Chunk)>>;
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
