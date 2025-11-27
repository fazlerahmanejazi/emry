use coderet_context::RepoContext;
use coderet_context::types::SymbolHit;
use anyhow::Result;
use coderet_core::models::{Language, ScoredChunk};
use coderet_core::ranking::RankConfig;
use coderet_pipeline::manager::IndexManager;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize; // Added import

pub struct Search {
    ctx: Arc<RepoContext>,
    manager: Arc<IndexManager>,
}

#[derive(Debug, Clone, Serialize)] // Added Serialize
pub struct SearchResult {
    pub chunks: Vec<ScoredChunk>,
    pub symbols: Vec<SymbolHit>,
}

impl Search {
    pub fn new(ctx: Arc<RepoContext>, manager: Arc<IndexManager>) -> Self {
        Self { ctx, manager }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<SearchResult> {
        let cfg = rank_cfg(&self.ctx.config);
        let chunks = self
            .manager
            .search_ranked(query, limit, Some(cfg))
            .await?;

        let mut symbols = Vec::new();
        // Additionally search for exact symbol matches
        let graph = self.ctx.graph.read().unwrap();
        if let Ok(nodes) = graph.nodes_matching_label(query) {
            for node in nodes {
                // Filter for actual symbols (not files or other graph nodes that just contain the query)
                if node.kind != "symbol" {
                    continue;
                }
                let file_path = PathBuf::from(&node.file_path);
                let language = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(Language::from_extension)
                    .unwrap_or(Language::Unknown);

                // Try to get line numbers from the graph node itself
                let start_line = 0;
                let end_line = 0;

                symbols.push(SymbolHit {
                    name: node.label.clone(),
                    file_path: node.file_path.clone(),
                    language: language.clone(),
                    start_line,
                    end_line,
                    symbol: coderet_core::models::Symbol {
                        id: node.id.clone(),
                        name: node.label.clone(),
                        kind: node.kind.clone(),
                        file_path: file_path.clone(),
                        start_line,
                        end_line,
                        fqn: node.label.clone(),
                        language: language.clone(),
                        doc_comment: None,
                    },
                });
            }
        }

        Ok(SearchResult { chunks, symbols })
    }

    /// Heuristic entry points: symbols named main/run/serve.
    pub fn list_entry_points(&self) -> Result<Vec<SymbolHit>> {
        let mut out = Vec::new();
        let keywords = ["main", "run", "serve"];
        let graph = self.ctx.graph.read().unwrap();
        if let Ok(nodes) = graph.list_symbols() {
            for node in nodes {
                if !keywords.iter().any(|k| node.label.contains(k)) {
                    continue;
                }
                let file_path = PathBuf::from(&node.file_path);
                let language = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(Language::from_extension)
                    .unwrap_or(Language::Unknown);
                let kind = if node.kind.is_empty() {
                    "symbol".to_string()
                } else {
                    node.kind.clone()
                };

                let start_line = 0;
                let end_line = 0;

                out.push(SymbolHit {
                    name: node.label.clone(),
                    file_path: node.file_path.clone(),
                    language: language.clone(),
                    start_line,
                    end_line,
                    symbol: coderet_core::models::Symbol {
                        id: node.id.clone(),
                        name: node.label.clone(),
                        kind,
                        file_path: file_path.clone(),
                        start_line,
                        end_line,
                        fqn: node.label.clone(),
                        language: language.clone(),
                        doc_comment: None,
                    },
                });
            }
        }
        Ok(out)
    }
}

fn rank_cfg(config: &coderet_config::Config) -> RankConfig {
    RankConfig {
        lexical_weight: config.ranking.lexical,
        vector_weight: config.ranking.vector,
        graph_weight: config.ranking.graph,
        symbol_weight: config.ranking.symbol,
        graph_max_depth: config.graph.max_depth,
        graph_decay: config.graph.decay,
        graph_path_weight: config.graph.path_weight,
        bm25_k1: config.bm25.k1,
        bm25_b: config.bm25.b,
        bm25_avg_len: config.bm25.avg_len,
        edge_weights: config.graph.edge_weights.clone(),
        summary_similarity_threshold: 0.25, // TODO: add to config
        summary_boost_weight: config.ranking.summary,
    }
}