use super::Embedder;
use anyhow::{anyhow, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct LocalEmbedder {
    model: TextEmbedding,
}

impl LocalEmbedder {
    pub fn new(model_name: Option<String>) -> Result<Self> {
        let embedding_model = match model_name.as_deref() {
            Some("BGESmallENV15") => EmbeddingModel::BGESmallENV15,
            Some("BGEBaseEN") => EmbeddingModel::BGEBaseENV15,
            Some("AllMiniLML6V2") => EmbeddingModel::AllMiniLML6V2,
            _ => EmbeddingModel::BGESmallENV15, // Default
        };

        let model = TextEmbedding::try_new(
            InitOptions::new(embedding_model).with_show_download_progress(true),
        )
        .map_err(|e| anyhow!("Failed to initialize local embedding model: {}", e))?;

        Ok(Self { model })
    }
}

impl Embedder for LocalEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let embeddings = self.model.embed(texts.to_vec(), None)
            .map_err(|e| anyhow!("Failed to generate embeddings: {}", e))?;
        Ok(embeddings)
    }
}
