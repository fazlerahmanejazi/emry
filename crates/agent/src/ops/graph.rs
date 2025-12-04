use crate::project::context::RepoContext;
use crate::project::types::{GraphEdge, GraphSubgraph};
use anyhow::{anyhow, Result};

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use emry_core::models::paths::Path;

use serde::Serialize;
use emry_store::SurrealGraphNode;

use futures::future::BoxFuture;

pub struct GraphTool {
    ctx: Arc<RepoContext>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum GraphDirection {
    In,
    Out,
    Both,
}

#[derive(Debug, Serialize)]
pub struct CandidateNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub file_path: String,
}

#[derive(Debug, Serialize)]
pub struct GraphResult {
    pub subgraph: GraphSubgraph,
    pub paths: Vec<Path>,
    pub candidates: Option<Vec<CandidateNode>>,  // None = success, Some = needs disambiguation
}

#[derive(Debug, Serialize)]
pub struct UsageSnippet {
    pub file_path: String,
    pub line_number: usize,
    pub code: String,
}

impl GraphTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    pub fn graph<'a>(
        &'a self,
        symbol: &'a str,
        direction: GraphDirection,
        max_hops: usize,
        file_filter: Option<&'a str>,
    ) -> BoxFuture<'a, Result<GraphResult>> {
        Box::pin(async move {
            match direction {
                GraphDirection::Out => self.graph_out(symbol, file_filter).await,
                GraphDirection::In => self.graph_in(symbol, max_hops, file_filter).await,
                GraphDirection::Both => self.graph_both(symbol, max_hops, file_filter).await,
            }
        })
    }

    async fn get_start_node_or_candidates(&self, symbol: &str, file_filter: Option<&str>) -> Result<(Option<SurrealGraphNode>, Option<Vec<CandidateNode>>)> {
        let store = self.ctx.surreal_store.as_ref()
            .ok_or_else(|| anyhow!("SurrealStore not initialized"))?;
        
        if let Ok(Some(n)) = store.get_node(symbol).await {
            return Ok((Some(n), None));
        }
        
        let matches = store.find_nodes_by_label(symbol, file_filter).await?;
        if matches.len() > 1 {
             let candidates = matches.into_iter().map(|n| CandidateNode {
                id: n.id.to_string(),
                label: n.label,
                kind: n.kind,
                file_path: n.file_path,
            }).collect();
            return Ok((None, Some(candidates)));
        }

        Ok((matches.into_iter().next(), None))
    }

    async fn graph_out(&self, symbol: &str, file_filter: Option<&str>) -> Result<GraphResult> {
        let (node, candidates) = self.get_start_node_or_candidates(symbol, file_filter).await?;
        if let Some(candidates) = candidates {
            return Ok(GraphResult { subgraph: GraphSubgraph { nodes: vec![], edges: vec![] }, paths: vec![], candidates: Some(candidates) });
        }
        let start_node = node.ok_or_else(|| anyhow!("Symbol '{}' not found.", symbol))?;
        let start_node_id = start_node.id.to_string();

        let mut subgraph = GraphSubgraph { nodes: Vec::new(), edges: Vec::new() };
        subgraph.nodes.push(Self::to_graph_node(start_node.clone()));

        let store = self.ctx.surreal_store.as_ref().unwrap();
        let edges = store.get_neighbors(&start_node_id, "out").await?;
        
        for edge in edges {
            subgraph.edges.push(GraphEdge {
                source: edge.source.to_string(),
                target: edge.target.to_string(),
                kind: edge.relation,
            });
            if let Ok(Some(target)) = store.get_node_by_thing(&edge.target).await {
                subgraph.nodes.push(Self::to_graph_node(target));
            }
        }
        
        Ok(GraphResult { subgraph, paths: vec![], candidates: None })
    }

    async fn graph_in(&self, symbol: &str, max_hops: usize, file_filter: Option<&str>) -> Result<GraphResult> {
        let (node, candidates) = self.get_start_node_or_candidates(symbol, file_filter).await?;
        if let Some(candidates) = candidates {
             return Ok(GraphResult { subgraph: GraphSubgraph { nodes: vec![], edges: vec![] }, paths: vec![], candidates: Some(candidates) });
        }
        let start_node = node.ok_or_else(|| anyhow!("Symbol '{}' not found.", symbol))?;
        
        let mut subgraph = GraphSubgraph { nodes: Vec::new(), edges: Vec::new() };
        let mut visited_nodes = HashSet::new();
        let mut q = VecDeque::new();

        subgraph.nodes.push(Self::to_graph_node(start_node.clone()));
        q.push_back((start_node.id.to_string(), 0));
        visited_nodes.insert(start_node.id.to_string());
        
        let store = self.ctx.surreal_store.as_ref().unwrap();

        while let Some((current_node_id, hops)) = q.pop_front() {
            if hops >= max_hops { continue; }

            let in_edges = store.get_neighbors(&current_node_id, "in").await?;
            for edge in in_edges {
                let source_id = edge.source.to_string();
                subgraph.edges.push(GraphEdge {
                    source: source_id.clone(),
                    target: current_node_id.clone(),
                    kind: edge.relation,
                });

                if let Ok(Some(source_node)) = store.get_node_by_thing(&edge.source).await {
                    subgraph.nodes.push(Self::to_graph_node(source_node));
                }

                if !visited_nodes.contains(&source_id) {
                    visited_nodes.insert(source_id.clone());
                    q.push_back((source_id, hops + 1));
                }
            }
        }

        Ok(GraphResult { subgraph, paths: vec![], candidates: None })
    }

    async fn graph_both(&self, symbol: &str, max_hops: usize, file_filter: Option<&str>) -> Result<GraphResult> {
        let out_res = self.graph_out(symbol, file_filter).await?;
        if out_res.candidates.is_some() { return Ok(out_res); }

        let in_res = self.graph_in(symbol, max_hops, file_filter).await?;
        if in_res.candidates.is_some() { return Ok(in_res); }

        let mut subgraph = GraphSubgraph { nodes: Vec::new(), edges: Vec::new() };
        
        let mut unique_nodes = std::collections::HashMap::new();
        for node in out_res.subgraph.nodes { unique_nodes.insert(node.id.clone(), node); }
        for node in in_res.subgraph.nodes { unique_nodes.insert(node.id.clone(), node); }
        subgraph.nodes = unique_nodes.into_values().collect();

        let mut unique_edges = HashSet::new();
        let mut final_edges = Vec::new();
        #[derive(Hash, Eq, PartialEq)]
        struct EdgeKey(String, String, String);

        for edge in out_res.subgraph.edges {
             if unique_edges.insert(EdgeKey(edge.source.clone(), edge.target.clone(), edge.kind.clone())) {
                final_edges.push(edge);
            }
        }
        for edge in in_res.subgraph.edges {
             if unique_edges.insert(EdgeKey(edge.source.clone(), edge.target.clone(), edge.kind.clone())) {
                final_edges.push(edge);
            }
        }
        subgraph.edges = final_edges;

        Ok(GraphResult { subgraph, paths: vec![], candidates: None })
    }

    fn to_graph_node(n: SurrealGraphNode) -> crate::project::types::GraphNode {
        crate::project::types::GraphNode {
            id: n.id.to_string(),
            kind: n.kind,
            label: n.label,
            file_path: n.file_path,
            canonical_id: Some(n.id.to_string()),
        }
    }

    pub async fn find_references(&self, symbol_id: &str) -> Result<Vec<SurrealGraphNode>> {
        let store = self.ctx.surreal_store.as_ref()
            .ok_or_else(|| anyhow!("SurrealStore not initialized"))?;
        store.find_references(symbol_id).await
    }

    pub async fn find_definition(&self, symbol_name: &str) -> Result<Vec<SurrealGraphNode>> {
        let store = self.ctx.surreal_store.as_ref()
            .ok_or_else(|| anyhow!("SurrealStore not initialized"))?;
        store.find_definition(symbol_name).await
    }

    pub async fn get_type_definition(&self, symbol_name: &str) -> Result<Option<SurrealGraphNode>> {
        let store = self.ctx.surreal_store.as_ref()
            .ok_or_else(|| anyhow!("SurrealStore not initialized"))?;
        store.get_type_definition(symbol_name).await
    }

    pub async fn find_usages(&self, symbol: &str) -> Result<Vec<UsageSnippet>> {
        let graph_result = self.graph(symbol, GraphDirection::In, 1, None).await?;
        
        if graph_result.candidates.is_some() {
            return Err(anyhow!("Symbol '{}' is ambiguous. Use 'inspect_graph' to disambiguate first.", symbol));
        }

        let mut snippets = Vec::new();
        
        for edge in &graph_result.subgraph.edges {
            if let Some(source_node) = graph_result.subgraph.nodes.iter().find(|n| n.id == edge.source) {
                 let path = std::path::PathBuf::from(&source_node.file_path);
                 let root = &self.ctx.root;
                 let full_path = root.join(&path);
                 
                 if full_path.exists() {
                     if let Ok(content) = std::fs::read_to_string(&full_path) {
                        let lines: Vec<&str> = content.lines().collect();
                        for (i, line) in lines.iter().enumerate() {
                            if line.contains(symbol) {
                                let start = i.saturating_sub(2);
                                let end = (i + 3).min(lines.len());
                                let snippet = lines[start..end].join("\n");
                                
                                snippets.push(UsageSnippet {
                                    file_path: source_node.file_path.clone(),
                                    line_number: i + 1,
                                    code: snippet,
                                });
                            }
                        }
                     }
                 }
            }
        }

        snippets.sort_by(|a, b| a.file_path.cmp(&b.file_path).then(a.line_number.cmp(&b.line_number)));
        snippets.dedup_by(|a, b| a.file_path == b.file_path && a.line_number == b.line_number);
        
        Ok(snippets)
    }
}
