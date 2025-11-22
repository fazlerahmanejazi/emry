use crate::structure::graph::{CodeGraph, NodeId, EdgeType};
use crate::paths::{Path, PathNode, PathEdge};
use crate::paths::scorer::PathScorer;
use std::collections::{HashMap, VecDeque};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

pub struct PathBuilderConfig {
    pub max_length: usize,
    pub max_paths: usize,
}

impl Default for PathBuilderConfig {
    fn default() -> Self {
        Self {
            max_length: 5,
            max_paths: 20,
        }
    }
}

pub struct PathBuilder<'a> {
    original_graph: &'a CodeGraph,
    pet_graph: petgraph::Graph<NodeId, EdgeType>,
    node_map: HashMap<NodeId, NodeIndex>,
}

impl<'a> PathBuilder<'a> {
    pub fn new(graph: &'a CodeGraph) -> Self {
        let (pet_graph, node_map) = graph.as_petgraph();
        Self {
            original_graph: graph,
            pet_graph,
            node_map,
        }
    }

    pub fn find_paths(&self, start_node_id: &NodeId, config: &PathBuilderConfig) -> Vec<Path> {
        let mut paths = Vec::new();
        
        let start_idx = match self.node_map.get(start_node_id) {
            Some(idx) => *idx,
            None => return paths,
        };

        // Queue stores: (Current Node Index, History of Node Indices, History of Edge Types)
        let mut queue: VecDeque<(NodeIndex, Vec<NodeIndex>, Vec<EdgeType>)> = VecDeque::new();
        queue.push_back((start_idx, vec![start_idx], vec![]));

        while let Some((current_idx, node_history, edge_history)) = queue.pop_front() {
            // If we have a valid path (length > 0), add it
            if !edge_history.is_empty() {
                let path = self.construct_path(&node_history, &edge_history);
                if let Some(mut p) = path {
                    p.score = PathScorer::score(&p);
                    paths.push(p);
                }
            }

            if paths.len() >= config.max_paths {
                break;
            }

            if edge_history.len() >= config.max_length {
                continue;
            }

            // Explore neighbors
            for edge in self.pet_graph.edges(current_idx) {
                let next_idx = edge.target();
                // Avoid cycles
                if node_history.contains(&next_idx) {
                    continue;
                }

                let mut new_nodes = node_history.clone();
                new_nodes.push(next_idx);

                let mut new_edges = edge_history.clone();
                new_edges.push(edge.weight().clone());

                queue.push_back((next_idx, new_nodes, new_edges));
            }
        }
        paths
    }

    fn construct_path(&self, node_indices: &[NodeIndex], edge_types: &[EdgeType]) -> Option<Path> {
        let mut path_nodes = Vec::new();
        for idx in node_indices {
            let node_id = self.pet_graph.node_weight(*idx)?;
            let original_node = self.original_graph.nodes.get(node_id)?;
            
            path_nodes.push(PathNode {
                node_id: original_node.id.0.clone(),
                kind: original_node.kind.clone(),
                name: original_node.label.clone(),
                file_path: original_node.file_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                start_line: original_node.start_line,
                end_line: original_node.end_line,
            });
        }

        let mut path_edges = Vec::new();
        for (i, edge_type) in edge_types.iter().enumerate() {
            let from_node = &path_nodes[i];
            let to_node = &path_nodes[i+1];
            path_edges.push(PathEdge {
                from_node: from_node.node_id.clone(),
                to_node: to_node.node_id.clone(),
                kind: edge_type.clone(),
            });
        }

        Some(Path::new(path_nodes, path_edges))
    }
}
