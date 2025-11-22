use crate::structure::graph::{CodeGraph, NodeId, EdgeType};
use crate::paths::{Path, PathNode, PathEdge};
use crate::paths::scorer::PathScorer;
use std::collections::{HashMap, VecDeque};

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
    graph: &'a CodeGraph,
    adjacency: HashMap<NodeId, Vec<(NodeId, EdgeType)>>,
}

impl<'a> PathBuilder<'a> {
    pub fn new(graph: &'a CodeGraph) -> Self {
        let mut adjacency = HashMap::new();
        for edge in &graph.edges {
            adjacency.entry(edge.source.clone())
                .or_insert_with(Vec::new)
                .push((edge.target.clone(), edge.kind.clone()));
        }
        Self { graph, adjacency }
    }

    pub fn find_paths(&self, start_node: &NodeId, config: &PathBuilderConfig) -> Vec<Path> {
        let mut paths = Vec::new();
        let mut queue = VecDeque::new();
        
        // Path state: (Current Node, History of Nodes, History of Edges)
        queue.push_back((start_node.clone(), vec![start_node.clone()], vec![]));

        while let Some((current, node_history, edge_history)) = queue.pop_front() {
            // If we have a valid path (length > 0), add it
            if !edge_history.is_empty() {
                 let path_nodes: Vec<PathNode> = node_history.iter().filter_map(|nid| {
                    self.graph.nodes.get(nid).map(|n| PathNode {
                        node_id: n.id.0.clone(),
                        kind: format!("{:?}", n.kind),
                        name: n.label.clone(),
                        file_path: n.file_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                        start_line: n.start_line,
                        end_line: n.end_line,
                    })
                }).collect();

                let path_edges: Vec<PathEdge> = node_history.windows(2).zip(edge_history.iter()).map(|(pair, kind)| {
                    PathEdge {
                        from_node: pair[0].0.clone(),
                        to_node: pair[1].0.clone(),
                        kind: format!("{:?}", kind),
                    }
                }).collect();
                
                if path_nodes.len() == node_history.len() {
                    let mut path = Path::new(path_nodes, path_edges);
                    path.score = PathScorer::score(&path);
                    paths.push(path);
                }
            }

            if paths.len() >= config.max_paths {
                break;
            }

            if edge_history.len() >= config.max_length {
                continue;
            }

            if let Some(neighbors) = self.adjacency.get(&current) {
                for (next_node, edge_kind) in neighbors {
                    // Avoid cycles
                    if node_history.contains(next_node) {
                        continue;
                    }

                    let mut new_nodes = node_history.clone();
                    new_nodes.push(next_node.clone());
                    
                    let mut new_edges = edge_history.clone();
                    new_edges.push(edge_kind.clone());

                    queue.push_back((next_node.clone(), new_nodes, new_edges));
                }
            }
        }
        paths
    }
}
