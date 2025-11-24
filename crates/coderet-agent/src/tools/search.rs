use crate::context::RepoContext;
use crate::types::{ChunkHit, SymbolHit};
use anyhow::Result;
use coderet_core::models::Language;
use coderet_core::ranking::RankConfig;
use std::path::PathBuf;
use std::sync::Arc;

use super::SearchToolTrait;

pub struct SearchTool {
    ctx: Arc<RepoContext>,
}

impl SearchTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    /// Hybrid search with optional keyword emphasis (keywords are appended to the query).
    pub async fn search_chunks_with_keywords(
        &self,
        query: &str,
        keywords: &[String],
        top_k: usize,
    ) -> Result<Vec<ChunkHit>> {
        let mut q = query.to_string();
        if !keywords.is_empty() {
            q.push(' ');
            q.push_str(&keywords.join(" "));
        }
        self.search_chunks(&q, top_k).await
    }

    pub async fn search_chunks(&self, query: &str, top_k: usize) -> Result<Vec<ChunkHit>> {
        let cfg = rank_cfg(&self.ctx.config);
        let results = self
            .ctx
            .manager
            .search_ranked(query, top_k, Some(cfg))
            .await?;

        Ok(results
            .into_iter()
            .map(|h| ChunkHit {
                score: h.score,
                lexical_score: h.lexical_score,
                vector_score: h.vector_score,
                graph_path: h.graph_path,
                chunk: h.chunk,
            })
            .collect())
    }

    /// Best-effort symbol lookup by substring match on label.
    pub fn search_symbols(&self, query: &str) -> Result<Vec<SymbolHit>> {
        let mut out = Vec::new();
        let needle = query.to_lowercase();
        if let Ok(nodes) = self.ctx.graph.list_symbols() {
            for node in nodes {
                if !node.label.to_lowercase().contains(&needle) {
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
                out.push(SymbolHit {
                    name: node.label.clone(),
                    file_path: node.file_path.clone(),
                    language: language.clone(),
                    start_line: 0,
                    end_line: 0,
                    symbol: coderet_core::models::Symbol {
                        id: node.id.clone(),
                        name: node.label.clone(),
                        kind,
                        file_path: file_path.clone(),
                        start_line: 0,
                        end_line: 0,
                        fqn: node.label.clone(),
                        language: language.clone(),
                        doc_comment: None,
                    },
                });
            }
        }
        Ok(out)
    }

    /// Heuristic entry points: symbols named main/run/serve.
    pub fn list_entry_points(&self) -> Result<Vec<SymbolHit>> {
        let mut out = Vec::new();
        let keywords = ["main", "run", "serve"];
        if let Ok(nodes) = self.ctx.graph.list_symbols() {
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
                out.push(SymbolHit {
                    name: node.label.clone(),
                    file_path: node.file_path.clone(),
                    language: language.clone(),
                    start_line: 0,
                    end_line: 0,
                    symbol: coderet_core::models::Symbol {
                        id: node.id.clone(),
                        name: node.label.clone(),
                        kind,
                        file_path: file_path.clone(),
                        start_line: 0,
                        end_line: 0,
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

#[async_trait::async_trait(?Send)]
impl SearchToolTrait for SearchTool {
    async fn search_chunks(&self, query: &str, top_k: usize) -> Result<Vec<ChunkHit>> {
        SearchTool::search_chunks(self, query, top_k).await
    }

    async fn search_chunks_with_keywords(
        &self,
        query: &str,
        keywords: &[String],
        top_k: usize,
    ) -> Result<Vec<ChunkHit>> {
        SearchTool::search_chunks_with_keywords(self, query, keywords, top_k).await
    }

    fn search_symbols(&self, name: &str) -> Result<Vec<SymbolHit>> {
        SearchTool::search_symbols(self, name)
    }

    fn list_entry_points(&self) -> Result<Vec<SymbolHit>> {
        SearchTool::list_entry_points(self)
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
