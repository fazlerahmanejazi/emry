use crate::config::{Config, SearchMode};
use crate::embeddings::Embedder;
use crate::index::lexical::LexicalIndex;
use crate::index::vector::VectorIndex;
use crate::models::Chunk;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Retriever {
    lexical_index: Arc<LexicalIndex>,
    vector_index: Arc<VectorIndex>,
    embedder: Arc<dyn Embedder + Send + Sync>, // Use trait object
    config: Config,
}

impl Retriever {
    pub fn new(
        lexical_index: Arc<LexicalIndex>,
        vector_index: Arc<VectorIndex>,
        embedder: Arc<dyn Embedder + Send + Sync>,
        config: Config,
    ) -> Self {
        Self {
            lexical_index,
            vector_index,
            embedder,
            config,
        }
    }

    pub async fn search(&self, query: &str, mode: SearchMode, top_k: usize) -> Result<Vec<(f32, Chunk)>> {
        // let mode = &self.config.search.default_mode; // Use passed mode
        // let top_k = self.config.search.default_top_k;

        match mode {
            SearchMode::Lexical => {
                let results = self.lexical_index.search(query, top_k)?;
                Ok(results)
            }
            SearchMode::Semantic => {
                let embedding = self.embedder.embed(&[query.to_string()])?.pop().unwrap();
                let results = self.vector_index.search(&embedding, top_k).await?;
                Ok(results)
            }
            SearchMode::Hybrid => {
                // 1. Lexical Search
                let lexical_results = self.lexical_index.search(query, top_k * 2)?; // Fetch more for fusion
                
                // 2. Semantic Search
                let embedding = self.embedder.embed(&[query.to_string()])?.pop().unwrap();
                let semantic_results = self.vector_index.search(&embedding, top_k * 2).await?;

                // 3. Fusion
                let mut scores: HashMap<String, f32> = HashMap::new();
                let mut chunks: HashMap<String, Chunk> = HashMap::new();

                // Normalize scores (simple max normalization)
                let max_lexical = lexical_results.iter().map(|(s, _)| *s).fold(0.0, f32::max);
                let max_semantic = semantic_results.iter().map(|(s, _)| *s).fold(0.0, f32::max);

                let lexical_weight = 0.5; // TODO: Config
                let semantic_weight = 0.5;

                for (score, chunk) in lexical_results {
                    let norm_score = if max_lexical > 0.0 { score / max_lexical } else { 0.0 };
                    *scores.entry(chunk.id.clone()).or_insert(0.0) += norm_score * lexical_weight;
                    chunks.entry(chunk.id.clone()).or_insert(chunk);
                }

                for (score, chunk) in semantic_results {
                    let norm_score = if max_semantic > 0.0 { score / max_semantic } else { 0.0 };
                    *scores.entry(chunk.id.clone()).or_insert(0.0) += norm_score * semantic_weight;
                    chunks.entry(chunk.id.clone()).or_insert(chunk);
                }

                let mut final_results: Vec<(f32, Chunk)> = scores.into_iter()
                    .filter_map(|(id, score)| chunks.remove(&id).map(|c| (score, c)))
                    .collect();

                final_results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                final_results.truncate(top_k);

                Ok(final_results)
            }
        }
    }
}
