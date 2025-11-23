use crate::paths::scorer::PathScorer;
use crate::paths::{Path, PathEdge, PathNode};
use crate::structure::graph::{CodeGraph, EdgeType, NodeId};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::{HashMap, HashSet, VecDeque};

pub struct PathBuilderConfig {
    pub max_length: usize,
    pub max_paths: usize,
    pub branch_factor: usize,
    pub direction: TraversalDirection,
    pub allowed_edge_types: Option<HashSet<EdgeType>>,
}

impl Default for PathBuilderConfig {
    fn default() -> Self {
        Self {
            max_length: 5,
            max_paths: 20,
            branch_factor: 5,
            direction: TraversalDirection::Both,
            allowed_edge_types: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TraversalDirection {
    Forward,
    Backward,
    Both,
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
        let mut seen_paths: HashSet<String> = HashSet::new();

        let start_idx = match self.node_map.get(start_node_id) {
            Some(idx) => *idx,
            None => return paths,
        };

        // Priority queue stores (negative score, current idx, node history, edge history)
        let mut frontier: BinaryHeap<(Reverse<i32>, NodeIndex, Vec<NodeIndex>, Vec<EdgeType>)> =
            BinaryHeap::new();
        frontier.push((Reverse(0), start_idx, vec![start_idx], vec![]));

        while let Some((Reverse(score_so_far), current_idx, node_history, edge_history)) =
            frontier.pop()
        {
            if !edge_history.is_empty() {
                if let Some(mut path) = self.construct_path(&node_history, &edge_history) {
                    path.score = PathScorer::score(&path);
                    let key = canonical_key(&path);
                    if seen_paths.insert(key) {
                        paths.push(path);
                    }
                    if paths.len() >= config.max_paths {
                        break;
                    }
                }
            }

            if edge_history.len() >= config.max_length {
                continue;
            }

            let neighbors = self.neighbors(current_idx, config);
            for (edge_kind, next_idx) in neighbors {
                if node_history.contains(&next_idx) {
                    continue;
                }

                let mut new_nodes = node_history.clone();
                new_nodes.push(next_idx);

                let mut new_edges = edge_history.clone();
                new_edges.push(edge_kind.clone());

                let prospective =
                    score_so_far + (PathScorer::edge_weight(&edge_kind) * 1000.0) as i32;
                frontier.push((Reverse(prospective), next_idx, new_nodes, new_edges));
            }
        }

        paths
    }

    pub fn find_paths_bidirectional(
        &self,
        start_id: &NodeId,
        end_id: &NodeId,
        config: &PathBuilderConfig,
    ) -> Vec<Path> {
        let start_idx = match self.node_map.get(start_id) {
            Some(idx) => *idx,
            None => return Vec::new(),
        };
        let end_idx = match self.node_map.get(end_id) {
            Some(idx) => *idx,
            None => return Vec::new(),
        };

        if start_idx == end_idx {
            return Vec::new();
        }

        // Forward search from start
        let mut forward_visited: HashMap<NodeIndex, (Vec<NodeIndex>, Vec<EdgeType>)> = HashMap::new();
        let mut forward_queue: VecDeque<(NodeIndex, Vec<NodeIndex>, Vec<EdgeType>)> = VecDeque::new();
        forward_queue.push_back((start_idx, vec![start_idx], vec![]));
        forward_visited.insert(start_idx, (vec![start_idx], vec![]));

        // Backward search from end
        let mut backward_visited: HashMap<NodeIndex, (Vec<NodeIndex>, Vec<EdgeType>)> = HashMap::new();
        let mut backward_queue: VecDeque<(NodeIndex, Vec<NodeIndex>, Vec<EdgeType>)> = VecDeque::new();
        backward_queue.push_back((end_idx, vec![end_idx], vec![]));
        backward_visited.insert(end_idx, (vec![end_idx], vec![]));

        let mut paths = Vec::new();
        let mut seen_paths: HashSet<String> = HashSet::new();

        // Expand layer by layer
        for _depth in 0..config.max_length {
            // Expand Forward
            let level_size = forward_queue.len();
            for _ in 0..level_size {
                if let Some((curr, history, edges)) = forward_queue.pop_front() {
                    // Check intersection
                    if let Some((back_history, back_edges)) = backward_visited.get(&curr) {
                        // Merge paths
                        let mut full_nodes = history.clone();
                        // back_history is [end, ..., curr], we need [curr, ..., end] but skipping curr
                        let mut rev_back = back_history.clone();
                        rev_back.reverse();
                        if rev_back.len() > 1 {
                             full_nodes.extend_from_slice(&rev_back[1..]);
                        }

                        let mut full_edges = edges.clone();
                        // back_edges are [edge_to_end, ..., edge_to_curr], we need to reverse and invert direction if needed
                        // But wait, our backward search follows INCOMING edges, so the edge types are correct for A->B.
                        // Actually, if we traverse C <- B <- A, the edge is A->B.
                        // Let's verify edge direction.
                        
                        let mut rev_edges = back_edges.clone();
                        rev_edges.reverse();
                        full_edges.extend(rev_edges);

                        if let Some(mut path) = self.construct_path(&full_nodes, &full_edges) {
                            path.score = PathScorer::score(&path);
                            let key = canonical_key(&path);
                            if seen_paths.insert(key) {
                                paths.push(path);
                            }
                        }
                    }

                    if edges.len() >= config.max_length / 2 + 1 { continue; }

                    // Expand neighbors (Forward)
                    let fwd_config = PathBuilderConfig {
                        direction: TraversalDirection::Forward,
                        allowed_edge_types: config.allowed_edge_types.clone(),
                        ..*config
                    };
                    for (edge_kind, next_idx) in self.neighbors(curr, &fwd_config) {
                        if !forward_visited.contains_key(&next_idx) {
                            let mut new_hist = history.clone();
                            new_hist.push(next_idx);
                            let mut new_edges = edges.clone();
                            new_edges.push(edge_kind);
                            forward_visited.insert(next_idx, (new_hist.clone(), new_edges.clone()));
                            forward_queue.push_back((next_idx, new_hist, new_edges));
                        }
                    }
                }
            }
            
            if paths.len() >= config.max_paths { break; }

            // Expand Backward
            let level_size = backward_queue.len();
            for _ in 0..level_size {
                if let Some((curr, history, edges)) = backward_queue.pop_front() {
                    // Check intersection (already checked in forward, but check again for symmetry/timing)
                    if let Some((fwd_history, fwd_edges)) = forward_visited.get(&curr) {
                         // Merge paths (same logic)
                        let mut full_nodes = fwd_history.clone();
                        let mut rev_back = history.clone();
                        rev_back.reverse();
                        if rev_back.len() > 1 {
                             full_nodes.extend_from_slice(&rev_back[1..]);
                        }

                        let mut full_edges = fwd_edges.clone();
                        let mut rev_edges = edges.clone();
                        rev_edges.reverse();
                        full_edges.extend(rev_edges);

                        if let Some(mut path) = self.construct_path(&full_nodes, &full_edges) {
                            path.score = PathScorer::score(&path);
                            let key = canonical_key(&path);
                            if seen_paths.insert(key) {
                                paths.push(path);
                            }
                        }
                    }

                    if edges.len() >= config.max_length / 2 + 1 { continue; }

                    // Expand neighbors (Backward - follow INCOMING edges)
                    let back_config = PathBuilderConfig {
                        direction: TraversalDirection::Backward,
                        allowed_edge_types: config.allowed_edge_types.clone(),
                        ..*config
                    };
                    for (edge_kind, next_idx) in self.neighbors(curr, &back_config) {
                        if !backward_visited.contains_key(&next_idx) {
                            let mut new_hist = history.clone();
                            new_hist.push(next_idx);
                            let mut new_edges = edges.clone();
                            new_edges.push(edge_kind);
                            backward_visited.insert(next_idx, (new_hist.clone(), new_edges.clone()));
                            backward_queue.push_back((next_idx, new_hist, new_edges));
                        }
                    }
                }
            }
             if paths.len() >= config.max_paths { break; }
        }

        paths
    }

    fn neighbors(&self, idx: NodeIndex, config: &PathBuilderConfig) -> Vec<(EdgeType, NodeIndex)> {
        let mut out = Vec::new();

        let mut push_edges = |dir: Direction| {
            for edge in self.pet_graph.edges_directed(idx, dir) {
                let kind = edge.weight();
                if let Some(allowed) = &config.allowed_edge_types {
                    if !allowed.contains(kind) {
                        continue;
                    }
                }
                out.push((kind.clone(), edge.target()));
            }
        };

        match config.direction {
            TraversalDirection::Forward => push_edges(Direction::Outgoing),
            TraversalDirection::Backward => push_edges(Direction::Incoming),
            TraversalDirection::Both => {
                push_edges(Direction::Outgoing);
                push_edges(Direction::Incoming);
            }
        }

        // Sort by edge weight (importance) descending and cap by branch_factor
        out.sort_by(|(k1, _), (k2, _)| {
            PathScorer::edge_weight(k2)
                .partial_cmp(&PathScorer::edge_weight(k1))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(config.branch_factor);
        out
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
                file_path: original_node
                    .file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                start_line: original_node.start_line,
                end_line: original_node.end_line,
            });
        }

        let mut path_edges = Vec::new();
        for (i, edge_type) in edge_types.iter().enumerate() {
            let from_node = &path_nodes[i];
            let to_node = &path_nodes[i + 1];
            path_edges.push(PathEdge {
                from_node: from_node.node_id.clone(),
                to_node: to_node.node_id.clone(),
                kind: edge_type.clone(),
            });
        }

        Some(Path::new(path_nodes, path_edges))
    }
}

fn canonical_key(path: &Path) -> String {
    let ids: Vec<&str> = path.nodes.iter().map(|n| n.node_id.as_str()).collect();
    ids.join("->")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::graph::{CodeGraph, EdgeType, NodeType};
    use std::path::PathBuf;

    #[test]
    fn respects_allowed_edge_types_and_branch_factor() {
        let mut graph = CodeGraph::default();
        let file = PathBuf::from("f.rs");
        let a = NodeId("A".into());
        let b = NodeId("B".into());
        let c = NodeId("C".into());

        graph.add_node(
            a.clone(),
            NodeType::Function,
            "A".into(),
            None,
            None,
            Some(file.clone()),
            1,
            2,
            Vec::new(),
        );
        graph.add_node(
            b.clone(),
            NodeType::Function,
            "B".into(),
            None,
            None,
            Some(file.clone()),
            3,
            4,
            Vec::new(),
        );
        graph.add_node(
            c.clone(),
            NodeType::Function,
            "C".into(),
            None,
            None,
            Some(file),
            5,
            6,
            Vec::new(),
        );
        graph.add_edge(a.clone(), b.clone(), EdgeType::Calls);
        graph.add_edge(b.clone(), c.clone(), EdgeType::Imports);

        let builder = PathBuilder::new(&graph);
        let mut cfg = PathBuilderConfig::default();
        cfg.max_length = 3;
        cfg.max_paths = 5;
        cfg.branch_factor = 1;
        cfg.allowed_edge_types = Some([EdgeType::Calls].into_iter().collect());
        let paths = builder.find_paths(&a, &cfg);

        assert_eq!(paths.len(), 1, "only calls edge should be followed");
        assert_eq!(paths[0].edges.len(), 1);
        assert_eq!(paths[0].edges[0].kind, EdgeType::Calls);
        assert_eq!(paths[0].nodes[0].name, "A");
        assert_eq!(paths[0].nodes[1].name, "B");
    }
}
