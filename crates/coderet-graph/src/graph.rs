use anyhow::Result;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::HashMap;

use coderet_store::relation_store::RelationType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String, // "file", "symbol", "chunk"
    pub label: String,
    pub canonical_id: Option<String>,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: String, // "defines", "calls", "imports"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphNodeInfo {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphEdgeInfo {
    pub src: String,
    pub dst: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphSubgraph {
    pub nodes: Vec<GraphNodeInfo>,
    pub edges: Vec<GraphEdgeInfo>,
}

pub struct CodeGraph {
    nodes_tree: sled::Tree,
    edges_tree: sled::Tree,    // key: source:target
    outgoing_tree: sled::Tree, // key: source, value: Vec<target>
    incoming_tree: sled::Tree, // key: target, value: Vec<source>
    path_cache: std::sync::Mutex<(
        std::collections::HashMap<String, Vec<String>>,
        std::collections::VecDeque<String>,
        usize,
    )>,
}

impl CodeGraph {
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self {
            nodes_tree: db.open_tree("graph_nodes")?,
            edges_tree: db.open_tree("graph_edges")?,
            outgoing_tree: db.open_tree("graph_outgoing")?,
            incoming_tree: db.open_tree("graph_incoming")?,
            path_cache: std::sync::Mutex::new((
                std::collections::HashMap::new(),
                std::collections::VecDeque::new(),
                256,
            )),
        })
    }

    pub fn add_node(&self, node: GraphNode) -> Result<()> {
        let bytes = bincode::serialize(&node)?;
        self.nodes_tree.insert(node.id.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn add_edge(&self, source: &str, target: &str, kind: &str) -> Result<()> {
        let edge = Edge {
            source: source.to_string(),
            target: target.to_string(),
            kind: kind.to_string(),
        };
        let edge_key = format!("{}:{}", source, target);
        self.edges_tree
            .insert(edge_key.as_bytes(), bincode::serialize(&edge)?)?;

        // Update adjacency lists
        self.add_to_adjacency(&self.outgoing_tree, source, target)?;
        self.add_to_adjacency(&self.incoming_tree, target, source)?;

        Ok(())
    }

    fn add_to_adjacency(&self, tree: &sled::Tree, key: &str, value: &str) -> Result<()> {
        let mut list: Vec<String> = if let Some(bytes) = tree.get(key.as_bytes())? {
            bincode::deserialize(&bytes)?
        } else {
            Vec::new()
        };

        if !list.contains(&value.to_string()) {
            list.push(value.to_string());
            tree.insert(key.as_bytes(), bincode::serialize(&list)?)?;
        }
        Ok(())
    }

    pub fn get_neighbors(&self, id: &str) -> Result<Vec<GraphNode>> {
        let mut neighbors = Vec::new();
        if let Some(bytes) = self.outgoing_tree.get(id.as_bytes())? {
            let targets: Vec<String> = bincode::deserialize(&bytes)?;
            for target_id in targets {
                if let Some(node_bytes) = self.nodes_tree.get(target_id.as_bytes())? {
                    let node: GraphNode = bincode::deserialize(&node_bytes)?;
                    neighbors.push(node);
                }
            }
        }
        Ok(neighbors)
    }

    pub fn outgoing_edges_with_kind(&self, source: &str) -> Result<Vec<Edge>> {
        let mut edges = Vec::new();
        let prefix = format!("{}:", source);
        for item in self.edges_tree.scan_prefix(prefix.as_bytes()) {
            let (_, bytes) = item?;
            if let Ok(edge) = bincode::deserialize::<Edge>(&bytes) {
                edges.push(edge);
            }
        }
        Ok(edges)
    }

    pub fn get_node(&self, id: &str) -> Result<Option<GraphNode>> {
        if let Some(bytes) = self.nodes_tree.get(id.as_bytes())? {
            let node: GraphNode = bincode::deserialize(&bytes)?;
            Ok(Some(node))
        } else {
            Ok(None)
        }
    }

    pub fn outgoing_edges(&self, source: &str) -> Result<Vec<Edge>> {
        let mut edges = Vec::new();
        let prefix = format!("{}:", source);
        for item in self.edges_tree.scan_prefix(prefix.as_bytes()) {
            let (_, bytes) = item?;
            if let Ok(edge) = bincode::deserialize::<Edge>(&bytes) {
                edges.push(edge);
            }
        }
        Ok(edges)
    }

    pub fn list_symbols(&self) -> Result<Vec<GraphNode>> {
        let mut out = Vec::new();
        for item in self.nodes_tree.iter() {
            let (_, v) = item?;
            if let Ok(node) = bincode::deserialize::<GraphNode>(&v) {
                if node.kind == "symbol" {
                    out.push(node);
                }
            }
        }
        Ok(out)
    }

    pub fn nodes_matching_label(&self, needle: &str) -> Result<Vec<GraphNode>> {
        let lower = needle.to_lowercase();
        let mut out = Vec::new();
        for item in self.nodes_tree.iter() {
            let (_, v) = item?;
            if let Ok(node) = bincode::deserialize::<GraphNode>(&v) {
                if node
                    .canonical_id
                    .as_ref()
                    .map(|id| id.to_lowercase().contains(&lower))
                    .unwrap_or(false)
                    || node.label.to_lowercase().contains(&lower)
                {
                    out.push(node);
                }
            }
        }
        Ok(out)
    }

    pub fn shortest_distance(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
    ) -> Result<Option<usize>> {
        if from == to {
            return Ok(Some(0));
        }
        let mut queue = std::collections::VecDeque::new();
        let mut visited = std::collections::HashSet::new();
        queue.push_back((from.to_string(), 0usize));
        visited.insert(from.to_string());

        while let Some((node_id, dist)) = queue.pop_front() {
            if dist >= max_depth {
                continue;
            }
            if let Some(bytes) = self.outgoing_tree.get(node_id.as_bytes())? {
                let targets: Vec<String> = bincode::deserialize(&bytes)?;
                for t in targets {
                    if t == to {
                        return Ok(Some(dist + 1));
                    }
                    if visited.insert(t.clone()) {
                        queue.push_back((t, dist + 1));
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn shortest_path(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
    ) -> Result<Option<Vec<GraphNode>>> {
        if from == to {
            if let Some(bytes) = self.nodes_tree.get(from.as_bytes())? {
                let node: GraphNode = bincode::deserialize(&bytes)?;
                return Ok(Some(vec![node]));
            }
            return Ok(None);
        }

        let cache_key = format!("{}|{}|{}", from, to, max_depth);
        if let Some(cached) = self.get_cached_path(&cache_key)? {
            let mut nodes = Vec::new();
            for nid in cached {
                if let Some(nb) = self.nodes_tree.get(nid.as_bytes())? {
                    let n: GraphNode = bincode::deserialize(&nb)?;
                    nodes.push(n);
                }
            }
            if !nodes.is_empty() {
                return Ok(Some(nodes));
            }
        }

        let mut queue = std::collections::VecDeque::new();
        let mut parents: HashMap<String, Option<String>> = HashMap::new();
        queue.push_back(from.to_string());
        parents.insert(from.to_string(), None);

        while let Some(node_id) = queue.pop_front() {
            let depth = path_len(&parents, &node_id);
            if depth >= max_depth {
                continue;
            }
            if let Some(bytes) = self.outgoing_tree.get(node_id.as_bytes())? {
                let targets: Vec<String> = bincode::deserialize(&bytes)?;
                for t in targets {
                    if parents.contains_key(&t) {
                        continue;
                    }
                    parents.insert(t.clone(), Some(node_id.clone()));
                    if t == to {
                        let mut path_ids = vec![t];
                        let mut cur = node_id.clone();
                        while let Some(parent) = parents.get(&cur).and_then(|p| p.clone()) {
                            path_ids.push(cur);
                            cur = parent;
                        }
                        path_ids.push(from.to_string());
                        path_ids.reverse();

                        let mut nodes = Vec::new();
                        for nid in path_ids {
                            if let Some(nb) = self.nodes_tree.get(nid.as_bytes())? {
                                let n: GraphNode = bincode::deserialize(&nb)?;
                                nodes.push(n);
                            }
                        }
                        self.set_cached_path(cache_key, &nodes);
                        return Ok(Some(nodes));
                    }
                    queue.push_back(t);
                }
            }
        }
        Ok(None)
    }

    pub fn shortest_weighted_path(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
        weight: &dyn Fn(&str) -> f32,
    ) -> Result<Option<Vec<GraphNode>>> {
        if from == to {
            if let Some(bytes) = self.nodes_tree.get(from.as_bytes())? {
                let node: GraphNode = bincode::deserialize(&bytes)?;
                return Ok(Some(vec![node]));
            }
            return Ok(None);
        }

        let mut dist: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        let mut prev: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new(); // node -> (parent, edge kind)

        let mut heap: Vec<(f32, String)> = Vec::new();
        dist.insert(from.to_string(), 0.0);
        heap.push((0.0_f32, from.to_string()));

        while let Some((cost, node)) = pop_min(&mut heap) {
            if node == to {
                break;
            }
            let depth = prev_len(&prev, &node);
            if depth >= max_depth {
                continue;
            }
            let neighbors = self.outgoing_edges_with_kind(&node)?;
            for edge in neighbors {
                let w = weight(&edge.kind).max(0.0);
                let next = edge.target.clone();
                let next_cost = cost + w;
                let is_better = dist.get(&next).map(|c| next_cost < *c).unwrap_or(true);
                if is_better {
                    dist.insert(next.clone(), next_cost);
                    prev.insert(next.clone(), (node.clone(), edge.kind.clone()));
                    heap.push((next_cost, next));
                }
            }
        }

        if !dist.contains_key(to) {
            return Ok(None);
        }

        let mut path_ids = Vec::new();
        let mut cur = to.to_string();
        path_ids.push(cur.clone());
        while let Some((p, _)) = prev.get(&cur) {
            cur = p.clone();
            path_ids.push(cur.clone());
            if cur == from {
                break;
            }
        }
        path_ids.reverse();

        let mut nodes = Vec::new();
        for nid in path_ids {
            if let Some(nb) = self.nodes_tree.get(nid.as_bytes())? {
                let n: GraphNode = bincode::deserialize(&nb)?;
                nodes.push(n);
            }
        }
        if nodes.is_empty() {
            return Ok(None);
        }
        Ok(Some(nodes))
    }

    /// Collect neighbor node ids up to `max_hops`, optionally filtering by edge kind.
    pub fn neighbors_filtered(
        &self,
        start: &str,
        kinds: &[String],
        max_hops: usize,
    ) -> Result<Vec<GraphNode>> {
        let mut out = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        visited.insert(start.to_string());
        queue.push_back((start.to_string(), 0usize));
        while let Some((cur, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }
            for edge in self.outgoing_edges(&cur)? {
                if !kinds.is_empty() && !kinds.contains(&edge.kind) {
                    continue;
                }
                if visited.insert(edge.target.clone()) {
                    if let Some(node) = self.get_node(&edge.target)? {
                        out.push(node.clone());
                        queue.push_back((edge.target.clone(), depth + 1));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Return shortest paths (by hop count) between two nodes as lists of node ids.
    pub fn shortest_paths_ids(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
    ) -> Result<Vec<Vec<String>>> {
        let mut out = Vec::new();
        if let Some(path) = self.shortest_path(from, to, max_depth)? {
            out.push(path.into_iter().map(|n| n.id).collect());
        }
        if let Some(path) = self.shortest_weighted_path(from, to, max_depth, &|_| 1.0)? {
            let ids: Vec<String> = path.into_iter().map(|n| n.id).collect();
            if !out.contains(&ids) {
                out.push(ids);
            }
        }
        Ok(out)
    }

    /// Build a subgraph rooted at `node_id`, following relations up to `max_hops`.
    pub fn neighbors_subgraph(
        &self,
        node_id: &str,
        relation_types: &[RelationType],
        max_hops: u8,
    ) -> Result<GraphSubgraph> {
        let mut nodes: Vec<GraphNodeInfo> = Vec::new();
        let mut edges: Vec<GraphEdgeInfo> = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();

        if let Some(start) = self.get_node(node_id)? {
            nodes.push(GraphNodeInfo {
                id: start.id.clone(),
                kind: start.kind.clone(),
                label: start.label.clone(),
                file_path: Some(start.file_path.clone()),
            });
            visited.insert(start.id.clone());
            queue.push_back((start.id.clone(), 0u8));
        } else {
            return Ok(GraphSubgraph { nodes, edges });
        }

        while let Some((cur, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }
            for edge in self.outgoing_edges(&cur)? {
                if !relation_types.is_empty() && !relation_matches(&edge.kind, relation_types) {
                    continue;
                }
                edges.push(GraphEdgeInfo {
                    src: edge.source.clone(),
                    dst: edge.target.clone(),
                    relation: edge.kind.clone(),
                });
                if visited.insert(edge.target.clone()) {
                    if let Some(node) = self.get_node(&edge.target)? {
                        nodes.push(GraphNodeInfo {
                            id: node.id.clone(),
                            kind: node.kind.clone(),
                            label: node.label.clone(),
                            file_path: Some(node.file_path.clone()),
                        });
                        queue.push_back((edge.target.clone(), depth + 1));
                    }
                }
            }
        }

        Ok(GraphSubgraph { nodes, edges })
    }

    /// Spec-aligned alias for neighbor subgraphs.
    pub fn neighbors(
        &self,
        node_id: &str,
        relation_types: &[RelationType],
        max_hops: u8,
    ) -> Result<GraphSubgraph> {
        self.neighbors_subgraph(node_id, relation_types, max_hops)
    }

    /// Shortest paths that honor relation filters; returns node-id paths.
    pub fn shortest_paths_filtered(
        &self,
        from: &str,
        to: &str,
        relation_types: &[RelationType],
        max_depth: usize,
    ) -> Result<Vec<Vec<String>>> {
        let mut out = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        let mut parents: HashMap<String, Option<String>> = HashMap::new();
        queue.push_back(from.to_string());
        parents.insert(from.to_string(), None);

        while let Some(node_id) = queue.pop_front() {
            let depth = path_len(&parents, &node_id);
            if depth >= max_depth {
                continue;
            }
            for edge in self.outgoing_edges(&node_id)? {
                if !relation_types.is_empty() && !relation_matches(&edge.kind, relation_types) {
                    continue;
                }
                if parents.contains_key(&edge.target) {
                    continue;
                }
                parents.insert(edge.target.clone(), Some(node_id.clone()));
                if edge.target == to {
                    let mut path_ids = vec![edge.target.clone()];
                    let mut cur = node_id.clone();
                    while let Some(parent) = parents.get(&cur).and_then(|p| p.clone()) {
                        path_ids.push(cur.clone());
                        cur = parent;
                    }
                    path_ids.push(from.to_string());
                    path_ids.reverse();
                    out.push(path_ids);
                    continue;
                }
                queue.push_back(edge.target.clone());
            }
        }

        Ok(out)
    }

    /// Spec-aligned alias for filtered shortest paths.
    pub fn shortest_paths(
        &self,
        from: &str,
        to: &str,
        relation_types: &[RelationType],
        max_depth: usize,
    ) -> Result<Vec<Vec<String>>> {
        self.shortest_paths_filtered(from, to, relation_types, max_depth)
    }

    /// Delete all nodes associated with a file and their edges.
    pub fn delete_nodes_for_file(&self, file_path: &str) -> Result<()> {
        // Collect node ids to delete
        let mut to_delete = Vec::new();
        for item in self.nodes_tree.iter() {
            let (k, v) = item?;
            if let Ok(node) = bincode::deserialize::<GraphNode>(&v) {
                if node.file_path == file_path {
                    to_delete.push((k, node));
                }
            }
        }

        if to_delete.is_empty() {
            return Ok(());
        }

        // Remove nodes and associated edges/adjacency
        for (key, node) in to_delete {
            let node_id = node.id.clone();
            let _ = self.nodes_tree.remove(key);

            // Remove outgoing edges
            if let Some(out_bytes) = self.outgoing_tree.remove(node_id.as_bytes())? {
                let targets: Vec<String> = bincode::deserialize(&out_bytes)?;
                for tgt in targets {
                    let edge_key = format!("{}:{}", node_id, tgt);
                    let _ = self.edges_tree.remove(edge_key.as_bytes());
                    // Remove reverse link in incoming_tree
                    if let Some(mut incoming) = self.get_adjacent_mut(&self.incoming_tree, &tgt)? {
                        incoming.retain(|id| id != &node_id);
                        self.set_adjacent(&self.incoming_tree, &tgt, incoming)?;
                    }
                }
            }

            // Remove incoming edges
            if let Some(in_bytes) = self.incoming_tree.remove(node_id.as_bytes())? {
                let sources: Vec<String> = bincode::deserialize(&in_bytes)?;
                for src in sources {
                    let edge_key = format!("{}:{}", src, node_id);
                    let _ = self.edges_tree.remove(edge_key.as_bytes());
                    if let Some(mut outgoing) = self.get_adjacent_mut(&self.outgoing_tree, &src)? {
                        outgoing.retain(|id| id != &node_id);
                        self.set_adjacent(&self.outgoing_tree, &src, outgoing)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn get_adjacent_mut(&self, tree: &sled::Tree, key: &str) -> Result<Option<Vec<String>>> {
        if let Some(bytes) = tree.get(key.as_bytes())? {
            let list: Vec<String> = bincode::deserialize(&bytes)?;
            Ok(Some(list))
        } else {
            Ok(None)
        }
    }

    fn set_adjacent(&self, tree: &sled::Tree, key: &str, list: Vec<String>) -> Result<()> {
        if list.is_empty() {
            let _ = tree.remove(key.as_bytes())?;
        } else {
            tree.insert(key.as_bytes(), bincode::serialize(&list)?)?;
        }
        Ok(())
    }

    fn get_cached_path(&self, key: &str) -> Result<Option<Vec<String>>> {
        let guard = self.path_cache.lock().unwrap();
        if let Some(path) = guard.0.get(key) {
            return Ok(Some(path.clone()));
        }
        Ok(None)
    }

    fn set_cached_path(&self, key: String, nodes: &[GraphNode]) {
        if nodes.is_empty() {
            return;
        }
        let ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
        let mut guard = self.path_cache.lock().unwrap();
        let (map, order, cap) = &mut *guard;
        if map.contains_key(&key) {
            return;
        }
        map.insert(key.clone(), ids);
        order.push_back(key);
        if order.len() > *cap {
            if let Some(old) = order.pop_front() {
                map.remove(&old);
            }
        }
    }
}

fn path_len(parents: &HashMap<String, Option<String>>, node: &str) -> usize {
    let mut len = 0;
    let mut cur = node;
    while let Some(Some(parent)) = parents.get(cur) {
        len += 1;
        cur = parent;
    }
    len
}

fn prev_len(parents: &HashMap<String, (String, String)>, node: &str) -> usize {
    let mut len = 0;
    let mut cur = node;
    while let Some((parent, _)) = parents.get(cur) {
        len += 1;
        cur = parent;
    }
    len
}

fn pop_min(heap: &mut Vec<(f32, String)>) -> Option<(f32, String)> {
    if heap.is_empty() {
        return None;
    }
    let mut best_idx = 0;
    for i in 1..heap.len() {
        if heap[i].0 < heap[best_idx].0 {
            best_idx = i;
        }
    }
    Some(heap.swap_remove(best_idx))
}

fn relation_matches(kind: &str, filters: &[RelationType]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let normalized = kind.to_lowercase();
    filters.iter().any(|r| match r {
        RelationType::Calls => normalized == "calls",
        RelationType::Imports => normalized == "imports",
        RelationType::Defines => normalized == "defines",
    })
}
