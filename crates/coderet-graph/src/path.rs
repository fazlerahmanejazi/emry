use crate::graph::{CodeGraph, GraphNode};
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Default, Clone)]
pub struct PathBuilderConfig {
    pub max_length: usize,
    pub max_paths: usize,
}

pub struct Path {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<String>, // edge kinds between nodes
}

pub struct PathBuilder<'a> {
    graph: &'a CodeGraph,
}

impl<'a> PathBuilder<'a> {
    pub fn new(graph: &'a CodeGraph) -> Self {
        Self { graph }
    }

    /// Find up to `max_paths` shortest paths (best-effort) from start to any target node.
    pub fn find_paths_to(
        &self,
        start: &str,
        targets: &[String],
        cfg: &PathBuilderConfig,
    ) -> Result<Vec<Path>> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let target_set: HashSet<String> = targets.iter().cloned().collect();
        let mut queue = VecDeque::new();
        let mut parents: HashMap<String, (String, String)> = HashMap::new(); // child -> (parent, edge_kind)

        queue.push_back(start.to_string());
        parents.insert(start.to_string(), (String::new(), String::new()));

        let mut paths = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            let depth = path_len(&parents, &node_id);
            if depth >= cfg.max_length {
                continue;
            }

            for edge in self.graph.outgoing_edges(&node_id)? {
                if parents.contains_key(&edge.target) {
                    continue;
                }
                parents.insert(edge.target.clone(), (node_id.clone(), edge.kind.clone()));

                if target_set.contains(&edge.target) {
                    if let Some(path) = self.build_path(&edge.target, &parents)? {
                        paths.push(path);
                        if paths.len() >= cfg.max_paths {
                            return Ok(paths);
                        }
                    }
                }
                queue.push_back(edge.target.clone());
            }
        }

        Ok(paths)
    }

    fn build_path(
        &self,
        end: &str,
        parents: &HashMap<String, (String, String)>,
    ) -> Result<Option<Path>> {
        let mut ids = Vec::new();
        let mut edge_kinds = Vec::new();
        let mut cur = end.to_string();
        ids.push(cur.clone());
        while let Some((parent, edge_kind)) = parents.get(&cur) {
            if parent.is_empty() {
                break;
            }
            edge_kinds.push(edge_kind.clone());
            cur = parent.clone();
            ids.push(cur.clone());
        }
        ids.reverse();
        edge_kinds.reverse();

        let mut nodes = Vec::new();
        for id in ids {
            if let Some(node) = self.graph.get_node(&id)? {
                nodes.push(node);
            }
        }
        if nodes.len() < 2 {
            return Ok(None);
        }
        Ok(Some(Path {
            nodes,
            edges: edge_kinds,
        }))
    }
}

fn path_len(parents: &HashMap<String, (String, String)>, node: &str) -> usize {
    let mut len = 0;
    let mut cur = node;
    while let Some((parent, _)) = parents.get(cur) {
        if parent.is_empty() {
            break;
        }
        len += 1;
        cur = parent;
    }
    len
}
