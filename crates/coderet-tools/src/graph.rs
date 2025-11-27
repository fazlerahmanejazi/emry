use coderet_context::RepoContext;
use coderet_context::types::{GraphEdge, GraphSubgraph};
use anyhow::{anyhow, Result};

use std::collections::{HashSet, VecDeque}; // Added imports
use std::sync::Arc;

use coderet_core::models::paths::Path; // Re-use core Path type

use serde::Serialize; // Added import

pub struct GraphTool {
    ctx: Arc<RepoContext>,
}

#[derive(Debug, Clone, Copy, Serialize)] // Added Serialize
pub enum GraphDirection {
    In, // Callers, usages
    Out, // Callees, dependencies
}

#[derive(Debug, Serialize)] // Added Serialize
pub struct GraphResult {
    pub subgraph: GraphSubgraph,
    pub paths: Vec<Path>, // Using coderet_core::models::paths::Path
}

impl GraphTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    pub fn graph(
        &self,
        symbol: &str,
        direction: GraphDirection,
        max_hops: usize,
    ) -> Result<GraphResult> {
        let mut subgraph = GraphSubgraph {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let paths = Vec::new(); // Paths are not directly returned by this tool for now

        // Resolve symbol to actual graph node ID
        let node_id = if let Some(node) = self.ctx.graph.nodes_matching_label(symbol)?.into_iter().next() {
            node.id
        } else if let Some(node) = self.ctx.graph.get_node(symbol)? { // Check if 'symbol' is already a valid node ID
            node.id
        }
        else {
            return Err(anyhow!("Symbol '{}' not found in graph.", symbol));
        };

        match direction {
            GraphDirection::Out => {
                let sub = self.ctx.graph.neighbors(&node_id, &[], max_hops as u8)?;
                // Convert from coderet_graph type to coderet_context type
                subgraph.nodes = sub.nodes.into_iter().map(|n| coderet_graph::graph::GraphNode {
                    id: n.id,
                    kind: n.kind,
                    label: n.label,
                    file_path: n.file_path.unwrap_or_default(),
                    canonical_id: None,
                }).collect();
                subgraph.edges = sub.edges.into_iter().map(|e| GraphEdge {
                    source: e.src,
                    target: e.dst,
                    kind: e.relation,
                }).collect();
            }
            GraphDirection::In => {
                // Manually build subgraph for incoming edges
                let mut visited_nodes = HashSet::new();
                let mut q = VecDeque::new();
                q.push_back((node_id.clone(), 0));
                visited_nodes.insert(node_id.clone());

                while let Some((current_node_id, hops)) = q.pop_front() {
                    if hops > max_hops { continue; }

                    if let Some(node) = self.ctx.graph.get_node(&current_node_id)? {
                        subgraph.nodes.push(coderet_graph::graph::GraphNode {
                            id: node.id,
                            kind: node.kind,
                            label: node.label,
                            file_path: node.file_path.clone(),
                            canonical_id: node.canonical_id,
                        });
                    }

                    if let Ok(incoming_relations) = self.ctx.relation_store.get_sources_for_target(&current_node_id) {
                        for (source_id, relation_type) in incoming_relations {
                            // source_id is String, relation_type is RelationType
                            if let Some(_source_node) = self.ctx.graph.get_node(&source_id)? {
                                subgraph.edges.push(GraphEdge {
                                    source: source_id.clone(),
                                    target: current_node_id.clone(),
                                    kind: format!("{:?}", relation_type).to_lowercase(),
                                });

                                if !visited_nodes.contains(&source_id) {
                                    visited_nodes.insert(source_id.clone());
                                    q.push_back((source_id, hops + 1));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(GraphResult { subgraph, paths })
    }
}

use crate::GraphToolTrait;

impl GraphToolTrait for GraphTool {
    fn graph(
        &self,
        symbol: &str,
        direction: GraphDirection,
        max_hops: usize,
    ) -> Result<GraphResult> {
        GraphTool::graph(self, symbol, direction, max_hops)
    }
}
