use crate::config::{Config, SearchMode};
use crate::embeddings::Embedder;
use crate::index::lexical::LexicalIndex;
use crate::index::vector::VectorIndex;
use crate::models::Chunk;
use anyhow::Result;
use crate::ranking::features::FeatureExtractor;
use crate::ranking::model::Ranker;
use crate::structure::index::SymbolIndex;
use crate::structure::graph::{CodeGraph, NodeId};
use crate::paths::{Path, builder::{PathBuilder, PathBuilderConfig}};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Retriever {
    lexical_index: Arc<LexicalIndex>,
    vector_index: Arc<VectorIndex>,
    embedder: Arc<dyn Embedder + Send + Sync>,
    symbol_index: Option<Arc<SymbolIndex>>,
    graph: Option<Arc<CodeGraph>>,
    ranker: Option<Box<dyn Ranker + Send + Sync>>,
    config: Config,
}

impl Retriever {
    pub fn new(
        lexical_index: Arc<LexicalIndex>,
        vector_index: Arc<VectorIndex>,
        embedder: Arc<dyn Embedder + Send + Sync>,
        symbol_index: Option<Arc<SymbolIndex>>,
        graph: Option<Arc<CodeGraph>>,
        ranker: Option<Box<dyn Ranker + Send + Sync>>,
        config: Config,
    ) -> Self {
        Self {
            lexical_index,
            vector_index,
            embedder,
            symbol_index,
            graph,
            ranker,
            config,
        }
    }

    pub async fn search(&self, query: &str, mode: SearchMode, top_k: usize) -> Result<Vec<(f32, Chunk)>> {
        // let mode = &self.config.search.default_mode; // Use passed mode
        // let top_k = self.config.search.default_top_k;

        let mut candidates = match mode {
            SearchMode::Lexical => {
                let results = self.lexical_index.search(query, top_k * 2)?; // Fetch more for re-ranking
                results
            }
            SearchMode::Semantic => {
                let embedding = self.embedder.embed(&[query.to_string()])?.pop().unwrap();
                let results = self.vector_index.search(&embedding, top_k * 2).await?;
                results
            }
            SearchMode::Hybrid => {
                // 1. Lexical Search
                let lexical_results = self.lexical_index.search(query, top_k * 2)?;
                
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

                scores.into_iter()
                    .filter_map(|(id, score)| chunks.remove(&id).map(|c| (score, c)))
                    .collect()
            }
        };

        // Re-ranking
        if let Some(ranker) = &self.ranker {
            let extractor = FeatureExtractor::new(self.symbol_index.as_deref());
            
            // We need original scores for features.
            // In candidates, we have a combined score or single score.
            // FeatureExtractor expects bm25 and vector scores separately.
            // This is tricky with the current architecture where we lost the separate scores in Hybrid.
            // For now, we'll use the candidate score as both or split if possible.
            // Ideally, we should keep track of separate scores.
            // Simplification: Use candidate score as "base score".
            
            let mut re_ranked = Vec::new();
            for (score, chunk) in candidates {
                // Approximation: if hybrid, score is mixed. If lexical, vector is 0.
                let (bm25, vector) = match mode {
                    SearchMode::Lexical => (score, 0.0),
                    SearchMode::Semantic => (0.0, score),
                    SearchMode::Hybrid => (score, score), // Rough approx
                };

                let features = extractor.extract(query, &chunk.content, bm25, vector);
                let new_score = ranker.score(&features);
                re_ranked.push((new_score, chunk));
            }
            candidates = re_ranked;
        }

        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(top_k);

        Ok(candidates)
    }

    pub async fn search_paths(&self, query: &str, mode: SearchMode, top_k: usize) -> Result<(Vec<(f32, Chunk)>, Vec<Path>)> {
        let chunks = self.search(query, mode, top_k).await?;
        
        let mut paths = Vec::new();
        if let Some(graph) = &self.graph {
            let builder = PathBuilder::new(graph);
            let config = PathBuilderConfig::default(); // TODO: Allow config override

            // Use top chunks as seeds
            for (_, chunk) in &chunks {
                // Naive mapping: Chunk -> File Node (since we don't have exact symbol ID in chunk yet)
                // Or try to find symbol by name if chunk has metadata.
                // For Phase 3, let's try to map file path to File Node.
                // And if we have symbol index, try to find symbol.
                
                // Strategy 1: File Node
                let file_id = NodeId(chunk.file_path.to_string_lossy().to_string());
                if graph.nodes.contains_key(&file_id) {
                    let mut new_paths = builder.find_paths(&file_id, &config);
                    paths.append(&mut new_paths);
                }

                // Strategy 2: Symbol Node (if chunk name matches a symbol)
                // This is heuristic.
                if let Some(symbol_index) = &self.symbol_index {
                     // If we can extract a likely symbol name from chunk...
                     // Or just search symbol index with query and use those as seeds?
                }
            }

            // Also use direct symbol hits from query as seeds
            if let Some(symbol_index) = &self.symbol_index {
                let symbols = symbol_index.search(query);
                for symbol in symbols {
                    let node_id = NodeId(symbol.id.clone());
                    if graph.nodes.contains_key(&node_id) {
                        let mut new_paths = builder.find_paths(&node_id, &config);
                        paths.append(&mut new_paths);
                    }
                }
            }
        }
        
        // Deduplicate paths by ID
        paths.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        paths.dedup_by(|a, b| a.id == b.id);
        paths.truncate(10); // Cap total paths

        Ok((chunks, paths))
    }
}
