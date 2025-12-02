use crate::project::context::RepoContext;
use crate::project::types::SymbolHit;
use anyhow::Result;
use emry_core::models::{Language, ScoredChunk};
use emry_engine::search::service::SearchService;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

pub struct Search {
    service: Arc<SearchService>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub chunks: Vec<ScoredChunk>,
    pub symbols: Vec<SymbolHit>,
}

impl Search {
    pub fn new(_ctx: Arc<RepoContext>, service: Arc<SearchService>) -> Self {
        Self { service }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<SearchResult> {
        let results = self.service.search(query, limit, None).await?;
        
        let chunks: Vec<ScoredChunk> = results.into_iter().map(|c| {
            ScoredChunk {
                chunk: emry_core::models::Chunk {
                    id: c.id.map(|t| t.to_string()).unwrap_or_default(),
                    file_path: std::path::PathBuf::from(c.file.id.to_string()),
                    start_line: c.start_line,
                    end_line: c.end_line,
                    content: c.content,
                    content_hash: "".to_string(),
                    embedding: c.embedding,
                    scope_path: c.scopes,
                    language: Language::Unknown,
                    start_byte: None,
                    end_byte: None,
                    node_type: "".to_string(),
                    parent_scope: None,
                },
                score: 1.0,
                lexical_score: None,
                vector_score: None,
                graph_boost: None,
                symbol_boost: None,
                graph_path: None,
                graph_distance: None,
            }
        }).collect();

        let mut symbols = Vec::new();
        // Additionally search for exact symbol matches
        // 2. Graph Search (Lexical match on symbols)
        let graph = self.service.store();
        if let Ok(nodes) = graph.find_nodes_by_label(query, None).await {
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
                    symbol: emry_core::models::Symbol {
                        id: node.id.to_string(),
                        name: node.label.clone(),
                        kind: node.kind.clone(),
                        file_path: file_path.clone(),
                        start_line,
                        end_line,
                        fqn: node.label.clone(),
                        language: language.clone(),
                        doc_comment: None,
                        parent_scope: None,
                    },
                });
            }
        }

        Ok(SearchResult { chunks, symbols })
    }

    pub async fn search_with_context(&self, query: &str, limit: usize, _smart: bool) -> Result<emry_core::models::ContextGraph> {
        // Use smart search if requested, but currently we rely on service defaults or CLI handling.
        self.service.search_with_context(query, limit, None).await
    }

    /// Heuristic entry points: symbols named main/run/serve.
    pub async fn list_entry_points(&self) -> Result<Vec<SymbolHit>> {
        let mut out = Vec::new();
        let keywords = ["main", "run", "serve"];
        let graph = self.service.store();
        if let Ok(nodes) = graph.list_all_symbols().await {
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
                    symbol: emry_core::models::Symbol {
                        id: node.id.to_string(),
                        name: node.label.clone(),
                        kind,
                        file_path: file_path.clone(),
                        start_line,
                        end_line,
                        fqn: node.label.clone(),
                        language: language.clone(),
                        doc_comment: None,
                        parent_scope: None,
                    },
                });
            }
        }
        Ok(out)
    }
}
