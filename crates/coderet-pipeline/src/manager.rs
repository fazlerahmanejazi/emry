use anyhow::Result;
use coderet_core::models::{Chunk, ScoredChunk};
use coderet_core::ranking::RankConfig;
use coderet_core::traits::Embedder;
use coderet_graph::graph::{CodeGraph, GraphNode};
use coderet_graph::path::{PathBuilder, PathBuilderConfig};
use coderet_index::lexical::LexicalIndex;
use coderet_index::summaries::SummaryIndex;
use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::content_store::ContentStore;
use coderet_store::file_blob_store::FileBlobStore;
use coderet_store::file_store::{FileMetadata, FileStore};
// use coderet_store::relation_store::RelationType; // Removed
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tantivy::IndexWriter;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub graph_boost: Option<f32>,
    pub graph_distance: Option<usize>,
    pub graph_path: Option<Vec<String>>,
    pub symbol_boost: Option<f32>,
    pub summary_score: Option<f32>,
    pub chunk: Chunk,
}

pub struct IndexManager {
    pub lexical: Arc<LexicalIndex>,
    pub vector: Arc<Mutex<VectorIndex>>, // VectorIndex is async and needs mutability for some ops
    pub embedder: Option<Arc<dyn Embedder + Send + Sync>>,
    pub file_store: Arc<FileStore>,
    pub chunk_store: Arc<ChunkStore>,
    pub content_store: Arc<ContentStore>,
    pub file_blob_store: Arc<FileBlobStore>,
    // pub relation_store: Arc<RelationStore>, // Removed
    pub graph: Arc<RwLock<CodeGraph>>,
    pub summary: Option<Arc<Mutex<SummaryIndex>>>,
}

impl IndexManager {
    pub fn new(
        lexical: Arc<LexicalIndex>,
        vector: Arc<Mutex<VectorIndex>>,
        embedder: Option<Arc<dyn Embedder + Send + Sync>>,
        file_store: Arc<FileStore>,
        chunk_store: Arc<ChunkStore>,
        content_store: Arc<ContentStore>,
        file_blob_store: Arc<FileBlobStore>,
        // relation_store: Arc<RelationStore>, // Removed
        graph: Arc<RwLock<CodeGraph>>,
        summary: Option<Arc<Mutex<SummaryIndex>>>,
    ) -> Self {
        Self {
            lexical,
            vector,
            embedder,
            file_store,
            chunk_store,
            content_store,
            file_blob_store,
            // relation_store,
            graph,
            summary,
        }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<(f32, Chunk)>> {
        let hits = self.search_hybrid(query, limit).await?;
        Ok(hits.into_iter().map(|hit| (hit.score, hit.chunk)).collect())
    }

    pub async fn search_hybrid(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let lexical_hits = self.lexical.search(query, limit)?;

        // Vector search is optional; fall back gracefully when no embedder is configured.
        let vector_hits = if let Some(embedder) = &self.embedder {
            let query_vec = embedder.embed(query).await?;
            let guard = self.vector.lock().await;
            guard.search(&query_vec, limit).await?
        } else {
            Vec::new()
        };

        let mut merged: HashMap<String, (Chunk, Option<f32>, Option<f32>)> = HashMap::new();

        for (score, chunk) in lexical_hits {
            let entry = merged
                .entry(chunk.id.clone())
                .or_insert((chunk, None, None));
            entry.1 = Some(score);
        }

        for (score, chunk) in vector_hits {
            let entry = merged
                .entry(chunk.id.clone())
                .or_insert((chunk, None, None));
            entry.2 = Some(score);
        }

        let max_lex = merged
            .values()
            .filter_map(|(_, lex, _)| *lex)
            .fold(0.0_f32, f32::max);
        let max_vec = merged
            .values()
            .filter_map(|(_, _, vec)| *vec)
            .fold(0.0_f32, f32::max);

        // Favor lexical slightly; fall back to lexical-only if no vector scores are present.
        let (lex_w, vec_w) = if max_vec > 0.0 {
            (0.6_f32, 0.4_f32)
        } else {
            (1.0, 0.0)
        };

        let mut hits: Vec<SearchHit> = merged
            .into_iter()
            .map(|(_, (chunk, lex, vec))| {
                let lex_norm = lex.map(|s| if max_lex > 0.0 { s / max_lex } else { 0.0 });
                let vec_norm = vec.map(|s| if max_vec > 0.0 { s / max_vec } else { 0.0 });
                let score = lex_norm.unwrap_or(0.0) * lex_w + vec_norm.unwrap_or(0.0) * vec_w;
                SearchHit {
                    score,
                    lexical_score: lex,
                    vector_score: vec,
                    graph_boost: None,
                    graph_distance: None,
                    graph_path: None,
                    symbol_boost: None,
                    summary_score: None,
                    chunk,
                }
            })
            .collect();

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub async fn search_ranked(
        &self,
        query: &str,
        limit: usize,
        rank_cfg: Option<RankConfig>,
    ) -> Result<Vec<ScoredChunk>> {
        let cfg = rank_cfg.unwrap_or_default();
        let mut hits = self.search_hybrid(query, limit).await?;
        let mut existing_ids: std::collections::HashSet<String> =
            hits.iter().map(|h| h.chunk.id.clone()).collect();

        // Add symbol-definition hits if symbol names match the query
        if cfg.symbol_weight > 0.0 {
            let graph = self.graph.read().unwrap();
            if let Ok(nodes) = graph.nodes_matching_label(query) {
                for node in nodes {
                    if node.kind != "symbol" {
                        continue;
                    }
                    if let Ok(edges) = graph.incoming_edges(&node.id) {
                        for edge in edges {
                            if edge.kind != "defines" {
                                continue;
                            }
                            let source = edge.source;
                            if existing_ids.contains(&source) {
                                continue;
                            }
                            if let Some(chunk) = self.chunk_from_store(&source)? {
                                existing_ids.insert(source.clone());
                                hits.push(SearchHit {
                                    score: cfg.symbol_weight,
                                    lexical_score: None,
                                    vector_score: None,
                                    graph_boost: None,
                                    graph_distance: None,
                                    graph_path: Some(vec![format!(
                                        "{} definesજી {}",
                                        chunk.file_path.display(),
                                        node.label
                                    )]),
                                    symbol_boost: Some(cfg.symbol_weight),
                                    summary_score: None,
                                    chunk,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Apply graph/path boost using richer traversal when configured.
        if cfg.graph_weight > 0.0 {
            let graph = self.graph.read().unwrap();
            let targets = graph.nodes_matching_label(query).unwrap_or_default();
            let target_ids: Vec<String> = targets.iter().map(|n| n.id.clone()).collect();
            let path_builder = PathBuilder::new(&graph);
            let path_cfg = PathBuilderConfig {
                max_length: cfg.graph_max_depth.max(1),
                max_paths: 4,
            };
            for hit in hits.iter_mut() {
                let mut symbol_match_boost: Option<f32> = None;
                if let Ok(edges) = graph.outgoing_edges(&hit.chunk.id) {
                    for edge in edges {
                        if edge.kind != "defines" {
                            continue;
                        }
                        let target = edge.target;
                        if let Some(node) = graph.get_node(&target)? {
                            if node.label.to_lowercase().contains(&query.to_lowercase()) {
                                symbol_match_boost = Some(cfg.symbol_weight);
                                break;
                            }
                        }
                    }
                }

                let mut start_nodes = self.start_nodes_for_chunk(&hit.chunk);
                if start_nodes.is_empty() {
                    start_nodes.push(hit.chunk.file_path.to_string_lossy().to_string());
                }

                let mut best_boost: Option<f32> = None;
                let mut best_path_labels: Option<Vec<String>> = None;
                let mut best_dist: Option<usize> = None;

                for start in start_nodes.iter() {
                    // Weighted shortest path across all targets
                    for target in &target_ids {
                        /*
                        // Shortest weighted path not yet implemented in new CodeGraph
                        if let Ok(Some(weighted_path)) = graph.shortest_weighted_path(
                            start,
                            target,
                            cfg.graph_max_depth,
                            &|k| edge_weight(k, &cfg.edge_weights),
                        ) {
                             // ...
                        }
                        */
                        // Use basic shortest path for now
                        if let Ok(Some(path)) = graph.shortest_path(start, target, cfg.graph_max_depth) {
                             let dist = path.len().saturating_sub(1);
                             let labels: Vec<String> = path.iter().map(|n| n.label.clone()).collect();
                             // Simple scoring
                             let path_score = score_path_labels(&labels, cfg.graph_decay, cfg.graph_path_weight);
                             if best_boost.map_or(true, |b| path_score > b) {
                                 best_boost = Some(path_score);
                                 best_dist = Some(dist);
                                 best_path_labels = Some(labels);
                             }
                        }
                    }
                    let paths = path_builder.find_paths_to(start, &target_ids, &path_cfg)?;
                    for path in paths {
                        let dist = path.nodes.len().saturating_sub(1);
                        let path_score = score_path(
                            &path,
                            cfg.graph_decay,
                            cfg.graph_path_weight,
                            &cfg.edge_weights,
                        );
                        if best_boost.map_or(true, |b| path_score > b) {
                            best_boost = Some(path_score);
                            best_dist = Some(dist);
                            best_path_labels = Some(describe_path(&path));
                        }
                    }
                }

                // Fallback: direct symbol match on defined symbols if no path was found.
                if best_boost.is_none() {
                if let Ok(edges) = graph.outgoing_edges(&hit.chunk.id) {
                    for edge in edges {
                        if edge.kind != "defines" {
                            continue;
                        }
                        let target = edge.target;
                            if let Some(node) = graph.get_node(&target)? {
                                if node.label.to_lowercase().contains(&query.to_lowercase()) {
                                    best_boost = Some(cfg.symbol_weight);
                                    best_dist = Some(1);
                                    best_path_labels = Some(vec![
                                        hit.chunk.file_path.to_string_lossy().to_string(),
                                        format!("defines {}", node.label),
                                    ]);
                                    break;
                                }
                            }
                        }
                    }
                }

                hit.graph_boost = best_boost;
                hit.graph_distance = best_dist;
                hit.graph_path = best_path_labels;
                hit.symbol_boost = symbol_match_boost;
            }
        }
        let mut ranked: Vec<ScoredChunk> = hits
            .into_iter()
            .map(|h| {
                let raw_lex = h.lexical_score.unwrap_or(0.0);
                let len = (h.chunk.end_line.saturating_sub(h.chunk.start_line) + 1) as f32;
                let avg_len = cfg.bm25_avg_len.max(1) as f32;
                let denom =
                    raw_lex + cfg.bm25_k1 * (1.0 - cfg.bm25_b + cfg.bm25_b * (len / avg_len));
                let lex = if denom > 0.0 {
                    raw_lex * (cfg.bm25_k1 + 1.0) / denom
                } else {
                    raw_lex
                };
                let vec = h.vector_score.unwrap_or(0.0);
                let score = lex * cfg.lexical_weight
                    + vec * cfg.vector_weight
                    + h.graph_boost.unwrap_or(0.0) * cfg.graph_weight
                    + h.symbol_boost.unwrap_or(0.0) * cfg.symbol_weight;
                ScoredChunk {
                    score,
                    lexical_score: h.lexical_score,
                    vector_score: h.vector_score,
                    graph_boost: h.graph_boost,
                    graph_distance: h.graph_distance,
                    graph_path: h.graph_path,
                    symbol_boost: h.symbol_boost,
                    summary_score: None,
                    chunk: h.chunk,
                }
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked.truncate(limit);
        Ok(ranked)
    }

    fn start_nodes_for_chunk(&self, chunk: &Chunk) -> Vec<String> {
        let mut nodes = Vec::new();
        let graph = self.graph.read().unwrap();
        
        if let Ok(edges) = graph.outgoing_edges(&chunk.id) {
            for edge in edges {
                if edge.kind == "defines" {
                    nodes.push(edge.target);
                }
            }
        }
        if graph.get_node(&chunk.id).unwrap_or(None).is_some() {
            nodes.push(chunk.id.clone());
        }
        if let Ok(Some(fid)) = self.file_store.get_file_id(&chunk.file_path) {
            nodes.push(format!("file:{}", fid));
        }
        let file_node = chunk.file_path.to_string_lossy().to_string();
        nodes.push(file_node);
        nodes.sort();
        nodes.dedup();
        nodes
    }

    fn chunk_from_store(&self, chunk_id: &str) -> Result<Option<Chunk>> {
        let stored = match self.chunk_store.get_chunk(chunk_id)? {
            Some(c) => c,
            None => return Ok(None),
        };
        let meta = match self.file_store.get_file_metadata(stored.file_id)? {
            Some(m) => m,
            None => return Ok(None),
        };
        let content = self
            .content_store
            .get(&stored.content_hash)?
            .or_else(|| self.file_blob_store.get_for_path(&meta.path).ok().flatten())
            .or_else(|| std::fs::read_to_string(&meta.path).ok())
            .unwrap_or_default();
        let lines: Vec<&str> = content.lines().collect();
        let start = stored.start_line.saturating_sub(1);
        let end = usize::min(lines.len(), stored.end_line);
        let snippet = if start < end && end <= lines.len() {
            lines[start..end].join("\n")
        } else {
            String::new()
        };
        let language = meta
            .path
            .extension()
            .and_then(|e| e.to_str())
            .map(coderet_core::models::Language::from_extension)
            .unwrap_or(coderet_core::models::Language::Unknown);
        Ok(Some(Chunk {
            id: stored.id,
            language,
            file_path: meta.path,
            start_line: stored.start_line,
            end_line: stored.end_line,
            start_byte: None,
            end_byte: None,
            node_type: stored.node_type,
            content_hash: stored.content_hash,
            content: snippet,
            embedding: None,
            parent_scope: None,
            scope_path: Vec::new(),
        }))
    }

    pub async fn begin_transaction(&self) -> Result<Transaction<'_>> {
        let lexical_writer = self.lexical.writer()?;
        Ok(Transaction {
            manager: self,
            lexical_writer,
            chunks_to_add: Vec::new(),
            files_to_update: Vec::new(),
            graph_nodes_to_add: Vec::new(),
            graph_edges_to_add: Vec::new(),
            chunks_to_delete: Vec::new(),
            file_nodes_to_delete: Vec::new(),
            content_puts: Vec::new(),
            file_blob_puts: Vec::new(),
            // relations_to_add: Vec::new(), // Removed
        })
    }
    pub async fn search_contextual(
        &self,
        query: &str,
        limit: usize,
        rank_cfg: Option<RankConfig>,
    ) -> Result<coderet_core::models::ContextualResult> {
        let cfg = rank_cfg.unwrap_or_default();

        // 1. Get Base Chunks (Ranked)
        let mut chunks = self.search_ranked(query, limit, Some(cfg.clone())).await?;

        // 2. Get Summaries (if enabled)
        let mut summaries = Vec::new();
        if let Some(sum_idx) = &self.summary {
            if let Some(embedder) = &self.embedder {
                let guard = sum_idx.lock().await;
                if let Ok(results) = guard.semantic_search(query, embedder.as_ref(), limit).await {
                    for (score, s) in results {
                        if score >= cfg.summary_similarity_threshold {
                            summaries.push(s);
                        }
                    }
                }
            }
        }

        // 3. Apply Summary Boost
        if !summaries.is_empty() && cfg.summary_boost_weight > 0.0 {
            for chunk in &mut chunks {
                // Simple overlap check: if chunk file path matches summary file path
                // In a real impl, we'd check line ranges too.
                for s in &summaries {
                    if let Some(s_path) = &s.file_path {
                        if s_path == &chunk.chunk.file_path {
                            // Boost!
                            chunk.score += cfg.summary_boost_weight;
                            chunk.summary_score =
                                Some(chunk.summary_score.unwrap_or(0.0) + cfg.summary_boost_weight);
                        }
                    }
                }
            }
            // Re-sort after boosting
            chunks.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // 4. Get Paths (Global Context)
        let mut paths = Vec::new();
        if cfg.graph_weight > 0.0 {
            let graph = self.graph.read().unwrap();
            let builder = PathBuilder::new(&graph);
            let path_cfg = PathBuilderConfig {
                max_length: cfg.graph_max_depth.max(1),
                max_paths: 5,
            };

            // Seeds: Top chunks + Query matching symbols
            let mut seeds = Vec::new();
            for c in chunks.iter().take(5) {
                seeds.push(c.chunk.id.clone());
            }
            if let Ok(symbols) = graph.nodes_matching_label(query) {
                for s in symbols.iter().take(5) {
                    seeds.push(s.id.clone());
                }
            }
            seeds.sort();
            seeds.dedup();

            // Find paths from seeds to other seeds or important nodes
            // For simplicity, just explore from seeds
            for seed in seeds {
                if let Ok(p) = builder.find_paths_to(&seed, &[], &path_cfg) {
                    // Convert graph::Path to models::paths::Path
                    for gp in p {
                        let nodes: Vec<String> = gp.nodes.iter().map(|n| n.label.clone()).collect();
                        let edges = gp.edges.clone();
                        paths.push(coderet_core::models::paths::Path {
                            nodes,
                            edges,
                            score: 1.0, // Placeholder score
                        });
                    }
                }
            }
        }

        Ok(coderet_core::models::ContextualResult {
            chunks,
            paths,
            summaries,
        })
    }
}

fn describe_path(path: &coderet_graph::path::Path) -> Vec<String> {
    let mut labels = Vec::new();
    for (idx, node) in path.nodes.iter().enumerate() {
        if idx < path.edges.len() {
            labels.push(format!(
                "{} -{}->જી {}",
                node.label,
                path.edges[idx],
                path.nodes[idx + 1].label
            ));
        } else {
            labels.push(node.label.clone());
        }
    }
    labels
}

fn score_path(
    path: &coderet_graph::path::Path,
    decay: f32,
    path_weight: f32,
    edge_weights: &std::collections::HashMap<String, f32>,
) -> f32 {
    let mut score = 0.0;
    for (i, edge) in path.edges.iter().enumerate() {
        let hop_decay = decay.powf(i as f32);
        score += edge_weight(edge, edge_weights) * hop_decay;
    }
    score * path_weight / (path.edges.len().max(1) as f32)
}

fn score_path_labels(labels: &[String], decay: f32, path_weight: f32) -> f32 {
    let mut score = 0.0;
    for (i, _) in labels.iter().enumerate() {
        let hop_decay = decay.powf(i as f32);
        score += hop_decay;
    }
    score * path_weight / (labels.len().max(1) as f32)
}

fn edge_weight(kind: &str, overrides: &std::collections::HashMap<String, f32>) -> f32 {
    if let Some(w) = overrides.get(kind) {
        return *w;
    }
    match kind {
        "defines" => 1.25,
        "calls" => 1.0,
        "imports" => 0.75,
        "contains" => 0.6,
        _ => 0.5,
    }
}

pub struct Transaction<'a> {
    manager: &'a IndexManager,
    lexical_writer: IndexWriter,
    chunks_to_add: Vec<Chunk>,
    files_to_update: Vec<FileMetadata>,
    graph_nodes_to_add: Vec<GraphNode>,
    graph_edges_to_add: Vec<(String, String, String)>,
    chunks_to_delete: Vec<String>,
    file_nodes_to_delete: Vec<String>,
    content_puts: Vec<(String, String)>,
    file_blob_puts: Vec<(std::path::PathBuf, String)>,
    // relations_to_add: Vec<(String, String, RelationType)>, // Removed
}

impl<'a> Transaction<'a> {
    pub fn add_chunk(&mut self, chunk: Chunk, file_id: u64) -> Result<()> {
        // Add to Lexical Buffer (Tantivy writer)
        self.manager
            .lexical
            .add_chunk(&mut self.lexical_writer, &chunk)?;

        // Add to Vector Buffer (memory)
        let mut vector_chunk = chunk.clone();
        vector_chunk.content.clear();
        self.chunks_to_add.push(vector_chunk);

        // Add to ChunkStore (Sled) - Immediate write for now, or could buffer
        self.manager.chunk_store.add_chunk(&chunk, file_id)?;

        Ok(())
    }

    pub fn update_file_metadata(&mut self, meta: FileMetadata) {
        self.files_to_update.push(meta);
    }

    pub fn add_graph_node(&mut self, node: GraphNode) {
        self.graph_nodes_to_add.push(node);
    }

    pub fn add_graph_edge(&mut self, source: String, target: String, kind: String) {
        self.graph_edges_to_add.push((source, target, kind));
    }

    pub fn add_relation(&mut self, _source: String, _target: String, _rel: String) {
        // No-op: RelationStore is being removed.
    }

    pub fn delete_chunks(&mut self, ids: Vec<String>) {
        self.chunks_to_delete.extend(ids);
    }

    pub fn delete_file_node(&mut self, file_path: String) {
        self.file_nodes_to_delete.push(file_path);
    }

    pub fn put_content(&mut self, hash: String, content: String) {
        self.content_puts.push((hash, content));
    }

    pub fn put_file_blob(&mut self, path: std::path::PathBuf, content: String) {
        self.file_blob_puts.push((path, content));
    }

    pub async fn commit(self) -> Result<()> {
        // 1. Commit Lexical Index
        self.manager.lexical.commit(self.lexical_writer)?;

        // 2. Delete stale chunks (lexical + vector + stores + relations + graph nodes)
        if !self.chunks_to_delete.is_empty() {
            self.manager.lexical.delete_chunks(&self.chunks_to_delete)?;
            let mut vector = self.manager.vector.lock().await;
            vector.delete_chunks(&self.chunks_to_delete).await?;
            for _cid in &self.chunks_to_delete {
                // let _ = self.manager.relation_store.delete_by_source(cid);
            }
            self.manager
                .chunk_store
                .delete_chunks(&self.chunks_to_delete)?;
        }

        // 2a. Content-addressable writes
        for (hash, content) in self.content_puts {
            self.manager.content_store.put(&hash, &content)?;
        }
        // 2b. File blobs
        for (path, content) in self.file_blob_puts {
            let _ = self.manager.file_blob_store.put(&path, &content)?;
        }

        // 3. Delete file nodes and graph edges
        {
            let mut graph = self.manager.graph.write().unwrap();
            for file_node in &self.file_nodes_to_delete {
                let _ = graph.delete_nodes_for_file(file_node);
            }
        }

        // 4. Commit Vector Index additions
        let mut vector = self.manager.vector.lock().await;
        vector.add_chunks(&self.chunks_to_add).await?;

        // 5. Commit File Metadata (Sled)
        for meta in self.files_to_update {
            self.manager
                .file_store
                .update_file_metadata(meta)?;
        }

        // 6. Commit Graph (In-Memory + Save)
        {
            let mut graph = self.manager.graph.write().unwrap();
            for node in self.graph_nodes_to_add {
                graph.add_node(node)?;
            }
            for (source, target, kind) in self.graph_edges_to_add {
                graph.add_edge(&source, &target, &kind)?;
            }
            graph.save()?; // SAVE THE GRAPH TO DISK
        }

        /*
        for (source, target, rel) in self.relations_to_add {
            self.manager
                .relation_store
                .add_relation(&source, &target, rel)?;
        }
        */

        Ok(())
    }
}
