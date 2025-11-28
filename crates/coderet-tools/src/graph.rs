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
    Both, // Both directions
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

        let graph = self.ctx.graph.read().unwrap();

        // Resolve symbol to actual graph node ID
        let node_id = if let Some(node) = graph.nodes_matching_label(symbol)?.into_iter().next() {
            node.id
        } else if let Some(node) = graph.get_node(symbol)? { // Check if 'symbol' is already a valid node ID
            node.id
        }
        else {
            return Err(anyhow!("Symbol '{}' not found in graph.", symbol));
        };

        match direction {
            GraphDirection::Out => {
                let sub = graph.get_neighbors(&node_id)?;
                // Convert from coderet_graph type to coderet_context type
                // Note: get_neighbors now returns Vec<GraphNode>, not a subgraph struct directly in new impl
                // We need to manually construct edges or update get_neighbors to return edges too.
                // For now, let's just get outgoing edges.
                
                // Add the center node
                if let Some(node) = graph.get_node(&node_id)? {
                     subgraph.nodes.push(coderet_graph::graph::GraphNode {
                        id: node.id,
                        kind: node.kind,
                        label: node.label,
                        file_path: node.file_path,
                        canonical_id: node.canonical_id,
                    });
                }

                let edges = graph.outgoing_edges(&node_id)?;
                for edge in edges {
                     subgraph.edges.push(GraphEdge {
                        source: edge.source,
                        target: edge.target.clone(),
                        kind: edge.kind,
                    });
                    if let Some(target_node) = graph.get_node(&edge.target)? {
                        subgraph.nodes.push(target_node);
                    }
                }
            }
            GraphDirection::In => {
                let mut visited_nodes = HashSet::new();
                let mut q = VecDeque::new();
                q.push_back((node_id.clone(), 0));
                visited_nodes.insert(node_id.clone());

                while let Some((current_node_id, hops)) = q.pop_front() {
                    if hops > max_hops { continue; }

                    // Add current node to result
                    if let Some(node) = graph.get_node(&current_node_id)? {
                        subgraph.nodes.push(coderet_graph::graph::GraphNode {
                            id: node.id.clone(),
                            kind: node.kind.clone(),
                            label: node.label.clone(),
                            file_path: node.file_path.clone(),
                            canonical_id: node.canonical_id.clone(),
                        });
                    }

                    // Get incoming edges from CodeGraph
                    let in_edges = graph.incoming_edges(&current_node_id)?;
                    for edge in in_edges {
                        // Chunk Skipping Logic (Incoming)
                        // If source is a chunk, we want to map it to the File that contains it.
                        let mut final_source_id = edge.source.clone();
                        let relation_kind = edge.kind.clone();

                        if let Some(source_node) = graph.get_node(&edge.source)? {
                            if source_node.kind == "chunk" {
                                // Try to find the file containing this chunk
                                let chunk_in_edges = graph.incoming_edges(&source_node.id)?;
                                for ce in chunk_in_edges {
                                    if ce.kind == "contains" {
                                        if let Some(file_node) = graph.get_node(&ce.source)? {
                                            if file_node.kind == "file" {
                                                final_source_id = file_node.id.clone();
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Add edge
                        subgraph.edges.push(GraphEdge {
                            source: final_source_id.clone(),
                            target: current_node_id.clone(),
                            kind: relation_kind,
                        });

                        // Add source node to queue if not visited
                        if !visited_nodes.contains(&final_source_id) {
                            visited_nodes.insert(final_source_id.clone());
                            q.push_back((final_source_id, hops + 1));
                        }
                    }
                }
            }
            GraphDirection::Both => {
                // Combine Out and In
                // 1. Outgoing
                let out_sub = self.graph(symbol, GraphDirection::Out, max_hops)?;
                subgraph.nodes.extend(out_sub.subgraph.nodes);
                subgraph.edges.extend(out_sub.subgraph.edges);

                // 2. Incoming
                let in_sub = self.graph(symbol, GraphDirection::In, max_hops)?;
                // Deduplicate nodes
                for node in in_sub.subgraph.nodes {
                    if !subgraph.nodes.iter().any(|n| n.id == node.id) {
                        subgraph.nodes.push(node);
                    }
                }
                // Deduplicate edges
                for edge in in_sub.subgraph.edges {
                    if !subgraph.edges.iter().any(|e| e.source == edge.source && e.target == edge.target && e.kind == edge.kind) {
                        subgraph.edges.push(edge);
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
