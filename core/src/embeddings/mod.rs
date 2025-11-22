use anyhow::Result;

pub trait Embedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub mod external;
pub mod ollama;

pub use external::ExternalEmbedder;
pub use ollama::OllamaEmbedder;
