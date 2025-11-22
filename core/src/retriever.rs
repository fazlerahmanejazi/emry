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

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub score: f32,
    pub lexical_score: f32,
    pub semantic_score: f32,
    pub lexical_score_raw: f32,
    pub semantic_score_raw: f32,
    pub lexical_score_norm: f32,
    pub semantic_score_norm: f32,
    pub lexical_weight: f32,
    pub semantic_weight: f32,
    pub chunk: Chunk,
}

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

    pub async fn search(&self, query: &str, mode: SearchMode, top_k: usize) -> Result<Vec<SearchResult>> {
        let mut candidates: Vec<SearchResult> = match mode {
            SearchMode::Lexical => {
                let results = self.lexical_index.search(query, top_k * 2)?; // Fetch more for re-ranking
                results
                    .into_iter()
                    .map(|(score, chunk)| {
                        let raw = score;
                        SearchResult {
                            score,
                            lexical_score: raw,
                            semantic_score: 0.0,
                            lexical_score_raw: raw,
                            semantic_score_raw: 0.0,
                            lexical_score_norm: 1.0,
                            semantic_score_norm: 0.0,
                            lexical_weight: 1.0,
                            semantic_weight: 0.0,
                            chunk,
                        }
                    })
                    .collect()
            }
            SearchMode::Semantic => {
                let embedding = self.embedder.embed(&[query.to_string()])?.pop().unwrap_or_default();
                let results = self.vector_index.search(&embedding, top_k * 2).await?;
                results
                    .into_iter()
                    .map(|(score, chunk)| {
                        let raw = score;
                        SearchResult {
                            score,
                            lexical_score: 0.0,
                            semantic_score: raw,
                            lexical_score_raw: 0.0,
                            semantic_score_raw: raw,
                            lexical_score_norm: 0.0,
                            semantic_score_norm: 1.0,
                            lexical_weight: 0.0,
                            semantic_weight: 1.0,
                            chunk,
                        }
                    })
                    .collect()
            }
            SearchMode::Hybrid => {
                // 1. Lexical Search
                let lexical_results = self.lexical_index.search(query, top_k * 2)?;
                
                // 2. Semantic Search
                let embedding = self.embedder.embed(&[query.to_string()])?.pop().unwrap_or_default();
                let semantic_results = self.vector_index.search(&embedding, top_k * 2).await?;

                // 3. Fusion
                let mut scores: HashMap<String, (f32, f32)> = HashMap::new(); // (lexical, semantic)
                let mut chunks: HashMap<String, Chunk> = HashMap::new();

                // Normalize scores (simple max normalization)
                let max_lexical = lexical_results.iter().map(|(s, _)| *s).fold(0.0, f32::max);
                let max_semantic = semantic_results.iter().map(|(s, _)| *s).fold(0.0, f32::max);

                let lexical_weight = 0.5; // TODO: Config
                let semantic_weight = 0.5;

                for (score, chunk) in lexical_results {
                    let norm_score = if max_lexical > 0.0 { score / max_lexical } else { 0.0 };
                    scores.entry(chunk.id.clone()).or_insert((0.0, 0.0)).0 = norm_score;
                    chunks.entry(chunk.id.clone()).or_insert(chunk);
                }

                for (score, chunk) in semantic_results {
                    let norm_score = if max_semantic > 0.0 { score / max_semantic } else { 0.0 };
                    scores.entry(chunk.id.clone()).or_insert((0.0, 0.0)).1 = norm_score;
                    chunks.entry(chunk.id.clone()).or_insert(chunk);
                }

                scores
                    .into_iter()
                    .filter_map(|(id, (lex_norm, sem_norm))| {
                        chunks.remove(&id).map(|c| SearchResult {
                            score: lex_norm * lexical_weight + sem_norm * semantic_weight,
                            lexical_score: lex_norm * lexical_weight,
                            semantic_score: sem_norm * semantic_weight,
                            lexical_score_raw: max_lexical,
                            semantic_score_raw: max_semantic,
                            lexical_score_norm: lex_norm,
                            semantic_score_norm: sem_norm,
                            lexical_weight,
                            semantic_weight,
                            chunk: c,
                        })
                    })
                    .collect()
            }
        };

        // Re-ranking
        if let Some(ranker) = &self.ranker {
            let extractor = FeatureExtractor::new(self.symbol_index.as_deref());
            let mut re_ranked = Vec::new();
            for mut res in candidates {
                let features = extractor.extract(query, &res.chunk.content, res.lexical_score, res.semantic_score);
                let new_score = ranker.score(&features);
                res.score = new_score;
                re_ranked.push(res);
            }
            candidates = re_ranked;
        }

        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(top_k);

        Ok(candidates)
    }

    pub async fn search_paths(&self, query: &str, mode: SearchMode, top_k: usize) -> Result<(Vec<SearchResult>, Vec<Path>)> {
        println!("DEBUG: Inside search_paths, calling search...");
        let chunks = self.search(query, mode, top_k).await?;
        println!("DEBUG: search returned {} chunks.", chunks.len());
        
        let mut paths = Vec::new();
        if let Some(graph) = &self.graph {
            let builder = PathBuilder::new(graph);
            let config = PathBuilderConfig::default();

            // 1. Select Seeds
            let chunk_data: Vec<Chunk> = chunks.iter().map(|r| r.chunk.clone()).collect();
            let seeds = crate::paths::selector::SeedSelector::select_seeds(graph, &chunk_data, 10);

            // 2. Build Paths
            for seed_id in seeds {
                let mut new_paths = builder.find_paths(&seed_id, &config);
                paths.append(&mut new_paths);
            }

            // 3. Also use direct symbol hits if available
            if let Some(symbol_index) = &self.symbol_index {
                let symbols = symbol_index.search(query);
                for symbol in symbols.iter().take(5) {
                    let node_id = NodeId(symbol.id.clone());
                    if graph.nodes.contains_key(&node_id) {
                        let mut new_paths = builder.find_paths(&node_id, &config);
                        paths.append(&mut new_paths);
                    }
                }
            }
        }
        
        // 4. Score and Deduplicate
        // Note: Heuristic scoring is already applied in PathBuilder via PathScorer::score
        // We just sort and dedup here.
        
        paths.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        paths.dedup_by(|a, b| a.id == b.id);
        paths.truncate(10);

        Ok((chunks, paths))
    }
}
