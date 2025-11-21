use anyhow::Result;

pub trait Embedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub mod local;
pub mod external;

pub use local::LocalEmbedder;
pub use external::ExternalEmbedder;
