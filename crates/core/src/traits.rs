
use anyhow::Result;

use async_trait::async_trait;



#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[async_trait]
pub trait LLM: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
}
