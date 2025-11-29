use anyhow::Result;
use emry_core::traits::Embedder;
use emry_store::{SurrealStore, ChunkRecord};
use std::sync::Arc;

pub struct SearchService {
    store: Arc<SurrealStore>,
    embedder: Option<Arc<dyn Embedder + Send + Sync>>,
}

impl SearchService {
    pub fn store(&self) -> &Arc<SurrealStore> {
        &self.store
    }

    pub fn new(
        store: Arc<SurrealStore>,
        embedder: Option<Arc<dyn Embedder + Send + Sync>>,
    ) -> Self {
        Self { store, embedder }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<ChunkRecord>> {
        // Hybrid Search Strategy:
        // 1. If embedder is available, do vector search.
        // 2. Always do FTS.
        // 3. Combine results (simple deduplication for now).
        
        let mut results = Vec::new();
        
        // Vector Search
        if let Some(embedder) = &self.embedder {
            if let Ok(embedding) = embedder.embed(query).await {
                match self.store.search_vector(embedding, limit).await {
                    Ok(vec_results) => {
                        // eprintln!("Vector search found {} results", vec_results.len());
                        results.extend(vec_results);
                    }
                    Err(e) => eprintln!("Vector search failed: {}", e),
                }
            }
        }
        
        // FTS
        match self.store.search_fts(query, limit).await {
            Ok(fts_results) => {
                // eprintln!("FTS search found {} results", fts_results.len());
                results.extend(fts_results);
            }
            Err(e) => eprintln!("FTS search failed: {}", e),
        }
        
        // Deduplicate by ID (if ID is available)
        // Since ChunkRecord ID is Option<Thing>, we might have issues if it's None (but it shouldn't be after read)
        // For MVP, just return all.
        // TODO: Implement proper RRF (Reciprocal Rank Fusion) or scoring.
        
        Ok(results)
    }
}
