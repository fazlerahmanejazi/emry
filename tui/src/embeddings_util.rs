use coderet_core::config::{EmbeddingBackend, EmbeddingsConfig};
use coderet_core::embeddings::{external::ExternalEmbedder, ollama::OllamaEmbedder, Embedder};
use std::env;
use std::sync::Arc;

// Choose embedder: external if OPENAI_API_KEY is set, otherwise Ollama.
pub fn select_embedder(cfg: &EmbeddingsConfig) -> Option<Arc<dyn Embedder + Send + Sync>> {
    if env::var("OPENAI_API_KEY").is_ok() {
        // If backend explicitly external, honor model_name; otherwise use provider default.
        let model_opt = if cfg.backend == EmbeddingBackend::External {
            Some(cfg.model_name.clone())
        } else {
            None
        };
        if let Ok(ext) = ExternalEmbedder::new(model_opt) {
            return Some(Arc::new(ext));
        }
    }
    // Ollama fallback
    if let Ok(ollama) = OllamaEmbedder::new(Some(cfg.model_name.clone())) {
        return Some(Arc::new(ollama));
    }
    None
}
