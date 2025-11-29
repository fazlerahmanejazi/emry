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
    In, // Callers, usages
    Out, // Callees, dependencies
    Both, // Both directions
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
            let mut subgraph = GraphSubgraph {
                nodes: Vec::new(),
                edges: Vec::new(),
            };
            let paths = Vec::new();

            let store = self.ctx.surreal_store.as_ref()
                .ok_or_else(|| anyhow!("SurrealStore not initialized"))?;

            // Resolve symbol to actual graph node ID
            // Try exact match first (if symbol is an ID)
            let node = if let Ok(Some(n)) = store.get_node(symbol).await {
                Some(n)
            } else {
                // Search by label WITH optional file filter
                let matches = store.find_nodes_by_label(symbol, file_filter).await?;
                
                // Handle disambiguation - return candidates if multiple matches
                if matches.len() > 1 {
                    let candidates = matches.into_iter().map(|n| CandidateNode {
                        id: n.id.to_string(),
                        label: n.label,
                        kind: n.kind,
                        file_path: n.file_path,
                    }).collect();
                    
                    return Ok(GraphResult {
                        subgraph: GraphSubgraph { nodes: vec![], edges: vec![] },
                        paths: vec![],
                        candidates: Some(candidates),
                    });
                }
                
                matches.into_iter().next()
            };

            let start_node = node.ok_or_else(|| anyhow!("Symbol '{}' not found in graph.", symbol))?;
            let start_node_id = start_node.id.to_string();

            // Helper to convert Surreal node to GraphNode
            let to_graph_node = |n: SurrealGraphNode| {
                crate::project::types::GraphNode {
                    id: n.id.to_string(),
                    kind: n.kind,
                    label: n.label,
                    file_path: n.file_path,
                    canonical_id: Some(n.id.to_string()),
                }
            };

            match direction {
                GraphDirection::Out => {
                    // Add center node
                    subgraph.nodes.push(to_graph_node(start_node.clone()));

                    // Get outgoing edges (1 hop for now, matching previous logic)
                    let edges = store.get_neighbors(&start_node_id, "out").await?;
                    for edge in edges {
                        subgraph.edges.push(GraphEdge {
                            source: edge.source.to_string(),
                            target: edge.target.to_string(),
                            kind: edge.relation,
                        });
                        
                        // Fetch target node details
                        if let Ok(Some(target)) = store.get_node_by_thing(&edge.target).await {
                            subgraph.nodes.push(to_graph_node(target));
                        }
                    }
                }
                GraphDirection::In => {
                    let mut visited_nodes = HashSet::new();
                    let mut q = VecDeque::new();
                    
                    // Add center node
                    subgraph.nodes.push(to_graph_node(start_node.clone()));
                    
                    q.push_back((start_node.id.to_string(), 0));
                    visited_nodes.insert(start_node.id.to_string());

                    while let Some((current_node_id, hops)) = q.pop_front() {
                        if hops >= max_hops { continue; }

                        // Get incoming edges
                        let in_edges = store.get_neighbors(&current_node_id, "in").await?;
                        for edge in in_edges {
                            let source_id = edge.source.to_string();
                            
                            // Add edge
                            subgraph.edges.push(GraphEdge {
                                source: source_id.clone(),
                                target: current_node_id.clone(),
                                kind: edge.relation,
                            });

                            // Fetch source node details
                            if let Ok(Some(source_node)) = store.get_node_by_thing(&edge.source).await {
                                 subgraph.nodes.push(to_graph_node(source_node));
                            }

                            // Add source node to queue if not visited
                            if !visited_nodes.contains(&source_id) {
                                visited_nodes.insert(source_id.clone());
                                q.push_back((source_id, hops + 1));
                            }
                        }
                    }
                }
                GraphDirection::Both => {
                // Combine Out and In
                let out_res = self.graph(symbol, GraphDirection::Out, max_hops, file_filter).await?;
                subgraph.nodes.extend(out_res.subgraph.nodes);
                subgraph.edges.extend(out_res.subgraph.edges);

                let in_res = self.graph(symbol, GraphDirection::In, max_hops, file_filter).await?;
                    
                    // Deduplicate
                    let mut unique_nodes = std::collections::HashMap::new();
                    for node in subgraph.nodes {
                        unique_nodes.insert(node.id.clone(), node);
                    }
                    for node in in_res.subgraph.nodes {
                        unique_nodes.insert(node.id.clone(), node);
                    }
                    subgraph.nodes = unique_nodes.into_values().collect();

                    let mut unique_edges = HashSet::new();
                    let mut final_edges = Vec::new();
                    // Helper for edge key
                    #[derive(Hash, Eq, PartialEq)]
                    struct EdgeKey(String, String, String);

                    for edge in subgraph.edges {
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
                }
            }

            Ok(GraphResult { subgraph, paths, candidates: None })
        })
    }
}

pub trait GraphToolTrait: Send + Sync {
    fn graph(
        &self,
        symbol: &str,
        direction: GraphDirection,
        max_hops: usize,
        file_filter: Option<&str>,
    ) -> BoxFuture<'static, Result<GraphResult>>;
}


impl GraphToolTrait for GraphTool {
    fn graph(
        &self,
        symbol: &str,
        direction: GraphDirection,
        max_hops: usize,
        file_filter: Option<&str>,
    ) -> BoxFuture<'static, Result<GraphResult>> {
        let ctx = self.ctx.clone();
        let symbol = symbol.to_string();
        let file_filter = file_filter.map(|s| s.to_string());
        Box::pin(async move {
             let tool = GraphTool::new(ctx);
             tool.graph(&symbol, direction, max_hops, file_filter.as_deref()).await
        })
    }
}
