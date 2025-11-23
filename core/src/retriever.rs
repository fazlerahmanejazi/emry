use crate::config::SearchMode;
use crate::embeddings::Embedder;
use crate::index::lexical::LexicalIndex;
use crate::index::vector::VectorIndex;
use crate::models::Chunk;
use crate::paths::{
    builder::{PathBuilder, PathBuilderConfig},
    Path,
};
use crate::ranking::features::FeatureExtractor;
use crate::ranking::model::Ranker;
use crate::structure::graph::{CodeGraph, EdgeType, NodeId};
use crate::structure::index::SymbolIndex;
use crate::summaries::index::SummaryIndex;
use crate::summaries::vector::SummaryVectorIndex;
use anyhow::Result;
use petgraph::visit::EdgeRef;
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
    pub graph_distance: Option<usize>,
    pub graph_boost: f32,
    pub chunk: Chunk,
}

pub struct Retriever {
    lexical_index: Arc<LexicalIndex>,
    vector_index: Arc<VectorIndex>,
    embedder: Arc<dyn Embedder + Send + Sync>,
    symbol_index: Option<Arc<SymbolIndex>>,
    summary_index: Option<Arc<SummaryIndex>>,
    graph: Option<Arc<CodeGraph>>,
    ranker: Option<Box<dyn Ranker + Send + Sync>>,
    summary_vector: Option<SummaryVectorIndex>,
}

impl Retriever {
    pub fn new(
        lexical_index: Arc<LexicalIndex>,
        vector_index: Arc<VectorIndex>,
        embedder: Arc<dyn Embedder + Send + Sync>,
        symbol_index: Option<Arc<SymbolIndex>>,
        summary_index: Option<Arc<SummaryIndex>>,
        graph: Option<Arc<CodeGraph>>,
        ranker: Option<Box<dyn Ranker + Send + Sync>>,
        summary_vector: Option<SummaryVectorIndex>,
    ) -> Self {
        Self {
            lexical_index,
            vector_index,
            embedder,
            symbol_index,
            summary_index,
            graph,
            ranker,
            summary_vector,
        }
    }

    pub async fn search(
        &self,
        query: &str,
        mode: SearchMode,
        top_k: usize,
    ) -> Result<Vec<SearchResult>> {
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
                            graph_distance: None,
                            graph_boost: 0.0,
                            chunk,
                        }
                    })
                    .collect()
            }
            SearchMode::Semantic => {
                let embedding = self
                    .embedder
                    .embed(&[query.to_string()])?
                    .pop()
                    .unwrap_or_default();
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
                            graph_distance: None,
                            graph_boost: 0.0,
                            chunk,
                        }
                    })
                    .collect()
            }
            SearchMode::Hybrid => {
                // 1. Lexical Search
                let lexical_results = self.lexical_index.search(query, top_k * 2)?;

                // 2. Semantic Search
                let embedding = self
                    .embedder
                    .embed(&[query.to_string()])?
                    .pop()
                    .unwrap_or_default();
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
                    let norm_score = if max_lexical > 0.0 {
                        score / max_lexical
                    } else {
                        0.0
                    };
                    scores.entry(chunk.id.clone()).or_insert((0.0, 0.0)).0 = norm_score;
                    chunks.entry(chunk.id.clone()).or_insert(chunk);
                }

                for (score, chunk) in semantic_results {
                    let norm_score = if max_semantic > 0.0 {
                        score / max_semantic
                    } else {
                        0.0
                    };
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
                            graph_distance: None,
                            graph_boost: 0.0,
                            chunk: c,
                        })
                    })
                    .collect()
            }
        };

        if let Some(graph) = &self.graph {
            apply_graph_boost(graph, &mut candidates);
        }

        // Re-ranking
        if let Some(ranker) = &self.ranker {
            let extractor = FeatureExtractor::new(self.symbol_index.as_deref());
            let mut re_ranked = Vec::new();
            for mut res in candidates {
                let features = extractor.extract(
                    query,
                    &res.chunk.content,
                    res.lexical_score,
                    res.semantic_score,
                );
                let new_score = ranker.score(&features);
                res.score = new_score;
                re_ranked.push(res);
            }
            candidates = re_ranked;
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(top_k);

        Ok(candidates)
    }

    pub async fn search_with_summaries(
        &self,
        query: &str,
        mode: SearchMode,
        top_k: usize,
        boost_weight: f32,
        similarity_threshold: f32,
    ) -> Result<(Vec<SearchResult>, Vec<crate::summaries::index::Summary>)> {
        let mut code = self.search(query, mode.clone(), top_k).await?;
        let mut summary_hits = Vec::new();
        if let Some(vec_idx) = &self.summary_vector {
            let query_emb = self
                .embedder
                .embed(&[query.to_string()])?
                .pop()
                .unwrap_or_default();
            if !query_emb.is_empty() {
                if let Ok(sum_results) = vec_idx.search(&query_emb, top_k * 2).await {
                    for (score, s) in sum_results {
                        if score >= similarity_threshold {
                            let mut s_clone = s.clone();
                            s_clone.text = format!("Score {:.2}: {}", score, s_clone.text);
                            summary_hits.push(s_clone);
                        }
                    }
                    self.apply_summary_boost(&mut code, &summary_hits, boost_weight);
                }
            }
        }
        // Fallback to in-memory embeddings if no vector index
        else if let Some(sum_idx) = &self.summary_index {
            if let Ok(sum_results) = sum_idx.semantic_search(query, self.embedder.as_ref(), top_k) {
                for (score, s) in sum_results {
                    if score >= similarity_threshold {
                        let mut s_clone = s.clone();
                        s_clone.text = format!("Score {:.2}: {}", score, s_clone.text);
                        summary_hits.push(s_clone);
                    }
                }
                self.apply_summary_boost(&mut code, &summary_hits, boost_weight);
            }
        }
        Ok((code, summary_hits))
    }

    pub async fn search_paths(
        &self,
        query: &str,
        mode: SearchMode,
        top_k: usize,
    ) -> Result<(Vec<SearchResult>, Vec<Path>)> {
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

        paths.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        paths.dedup_by(|a, b| a.id == b.id);
        paths.truncate(10);

        Ok((chunks, paths))
    }
}

fn apply_graph_boost(
    graph: &crate::structure::graph::CodeGraph,
    candidates: &mut Vec<SearchResult>,
) {
    if candidates.is_empty() {
        return;
    }
    let (pet, node_map) = graph.as_petgraph();
    // seeds: top few candidate nodes
    let mut seed_idxs = Vec::new();
    for res in candidates.iter().take(5) {
        if let Some(node_id) = graph.find_node_for_chunk(&res.chunk) {
            if let Some(idx) = node_map.get(&node_id) {
                seed_idxs.push(*idx);
            }
        }
    }
    if seed_idxs.is_empty() {
        return;
    }

    // Gather path coverage from seeds to bias boosts
    let mut path_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();
    let builder = crate::paths::builder::PathBuilder::new(graph);
    let mut path_cfg = crate::paths::builder::PathBuilderConfig::default();
    path_cfg.max_length = 4;
    path_cfg.max_paths = 30;
    path_cfg.branch_factor = 5;
    for seed in seed_idxs.iter().take(3) {
        if let Some(node_id) = pet.node_weight(*seed) {
            let paths = builder.find_paths(node_id, &path_cfg);
            for p in paths {
                for n in p.nodes {
                    path_nodes.insert(n.node_id);
                }
            }
        }
    }

    for res in candidates.iter_mut() {
        if let Some(node_id) = graph.find_node_for_chunk(&res.chunk) {
            if let Some(idx) = node_map.get(&node_id) {
                if let Some(dist) = shortest_distance_to_seeds(&pet, *idx, &seed_idxs, 4) {
                    res.graph_distance = Some(dist);
                    let boost = match dist {
                        0 => 0.30,
                        1 => 0.20,
                        2 => 0.10,
                        3 => 0.05,
                        _ => 0.0,
                    };
                    let path_bonus = if path_nodes.contains(&node_id.0) {
                        0.05
                    } else {
                        0.0
                    };
                    let total = boost + path_bonus;
                    res.graph_boost = total;
                    res.score += total;
                }
            }
        }
    }
}

fn shortest_distance_to_seeds(
    graph: &petgraph::Graph<NodeId, EdgeType>,
    start: petgraph::graph::NodeIndex,
    seeds: &[petgraph::graph::NodeIndex],
    max_depth: usize,
) -> Option<usize> {
    use std::collections::VecDeque;
    let mut visited = std::collections::HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((start, 0usize));
    visited.insert(start);

    while let Some((idx, dist)) = queue.pop_front() {
        if seeds.contains(&idx) {
            return Some(dist);
        }
        if dist >= max_depth {
            continue;
        }
        for edge in graph.edges(idx) {
            let next = edge.target();
            if visited.insert(next) {
                queue.push_back((next, dist + 1));
            }
        }
    }
    None
}

fn overlap(chunk: &Chunk, summary: &crate::summaries::index::Summary) -> bool {
    // Prefer exact chunk linkage if present
    if let Some(target_id) = summary.target_id.as_str().split('#').next() {
        let _ = target_id;
    }
    if let (Some(s), Some(e)) = (summary.start_line, summary.end_line) {
        if let Some(fp) = &summary.file_path {
            if fp != &chunk.file_path {
                return false;
            }
        } else {
            return false;
        }
        let start = std::cmp::max(chunk.start_line, s);
        let end = std::cmp::min(chunk.end_line, e);
        return start <= end;
    }
    false
}

impl Retriever {
    fn apply_summary_boost(
        &self,
        code: &mut Vec<SearchResult>,
        summaries: &[crate::summaries::index::Summary],
        weight: f32,
    ) {
        if summaries.is_empty() || weight <= 0.0 {
            return;
        }
        for res in code.iter_mut() {
            if summaries.iter().any(|s| overlap(&res.chunk, s)) {
                res.score += weight;
            }
        }
        code.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}
