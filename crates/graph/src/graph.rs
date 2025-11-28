use anyhow::{Context, Result};
use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub kind: String, // "file", "symbol", "chunk"
    pub label: String,
    pub canonical_id: Option<String>,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeData {
    pub kind: String, // "defines", "calls", "imports"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: String,
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

#[derive(Serialize, Deserialize)]
pub struct CodeGraph {
    graph: StableGraph<GraphNode, EdgeData>,
    node_indices: HashMap<String, NodeIndex>,
    // Secondary indices for performance
    files_to_nodes: HashMap<String, Vec<NodeIndex>>,
    symbol_nodes: Vec<NodeIndex>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl CodeGraph {
    pub fn new(path: PathBuf) -> Self {
        Self {
            graph: StableGraph::new(),
            node_indices: HashMap::new(),
            files_to_nodes: HashMap::new(),
            symbol_nodes: Vec::new(),
            path: Some(path),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new(path.to_path_buf()));
        }
        let file = File::open(path).context("failed to open graph file")?;
        let reader = BufReader::new(file);
        let mut graph: CodeGraph = bincode::deserialize_from(reader).context("failed to deserialize graph")?;
        graph.path = Some(path.to_path_buf());
        
        // Rebuild indices if they are empty (backward compatibility or if not serialized)
        // Note: We added them to the struct, so if we load an old graph, bincode might fail 
        // unless we handle versioning. But since we control the format, let's assume we might need to rebuild
        // or that the user will re-index.
        // Actually, bincode is strict. If the struct changed, old files won't load.
        // We might need to handle migration or just accept that the graph needs to be rebuilt.
        // For now, let's assume fresh start or compatible struct.
        // If we wanted to be safe, we'd use a versioned struct or rebuild indices here.
        // Let's ensure indices are populated if they were skipped or empty.
        if graph.files_to_nodes.is_empty() && !graph.node_indices.is_empty() {
             for idx in graph.graph.node_indices() {
                 if let Some(node) = graph.graph.node_weight(idx) {
                     graph.files_to_nodes.entry(node.file_path.clone()).or_default().push(idx);
                     if node.kind == "symbol" {
                         graph.symbol_nodes.push(idx);
                     }
                 }
             }
        }
        
        Ok(graph)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(path) = &self.path {
            let file = File::create(path).context("failed to create graph file")?;
            let writer = BufWriter::new(file);
            bincode::serialize_into(writer, self).context("failed to serialize graph")?;
        }
        Ok(())
    }

    pub fn add_node(&mut self, node: GraphNode) -> Result<()> {
        if self.node_indices.contains_key(&node.id) {
            return Ok(());
        }
        let id = node.id.clone();
        let file_path = node.file_path.clone();
        let is_symbol = node.kind == "symbol";
        
        let idx = self.graph.add_node(node);
        self.node_indices.insert(id, idx);
        
        // Update indices
        self.files_to_nodes.entry(file_path).or_default().push(idx);
        if is_symbol {
            self.symbol_nodes.push(idx);
        }
        
        Ok(())
    }

    pub fn add_edge(&mut self, source: &str, target: &str, kind: &str) -> Result<()> {
        let src_idx = match self.node_indices.get(source) {
            Some(idx) => *idx,
            None => return Ok(()), // Or error? For now, ignore if node missing to match old behavior
        };
        let tgt_idx = match self.node_indices.get(target) {
            Some(idx) => *idx,
            None => return Ok(()),
        };

        // Check if edge already exists
        if self.graph.edges_connecting(src_idx, tgt_idx).any(|e| e.weight().kind == kind) {
            return Ok(());
        }

        self.graph.add_edge(src_idx, tgt_idx, EdgeData { kind: kind.to_string() });
        Ok(())
    }

    pub fn remove_node(&mut self, id: &str) -> Result<()> {
        if let Some(idx) = self.node_indices.remove(id) {
            if let Some(node) = self.graph.node_weight(idx) {
                // Remove from file index
                if let Some(nodes) = self.files_to_nodes.get_mut(&node.file_path) {
                    if let Some(pos) = nodes.iter().position(|&x| x == idx) {
                        nodes.swap_remove(pos);
                    }
                }
                // Remove from symbol index
                if node.kind == "symbol" {
                    if let Some(pos) = self.symbol_nodes.iter().position(|&x| x == idx) {
                        self.symbol_nodes.swap_remove(pos);
                    }
                }
            }
            self.graph.remove_node(idx);
        }
        Ok(())
    }

    pub fn get_node(&self, id: &str) -> Result<Option<GraphNode>> {
        if let Some(idx) = self.node_indices.get(id) {
            Ok(self.graph.node_weight(*idx).cloned())
        } else {
            Ok(None)
        }
    }

    pub fn get_neighbors(&self, id: &str) -> Result<Vec<GraphNode>> {
        let mut neighbors = Vec::new();
        if let Some(idx) = self.node_indices.get(id) {
            for neighbor_idx in self.graph.neighbors_directed(*idx, Direction::Outgoing) {
                if let Some(node) = self.graph.node_weight(neighbor_idx) {
                    neighbors.push(node.clone());
                }
            }
        }
        Ok(neighbors)
    }

    pub fn outgoing_edges(&self, source: &str) -> Result<Vec<Edge>> {
        let mut edges = Vec::new();
        if let Some(src_idx) = self.node_indices.get(source) {
            for edge in self.graph.edges_directed(*src_idx, Direction::Outgoing) {
                if let Some(target_node) = self.graph.node_weight(edge.target()) {
                    edges.push(Edge {
                        source: source.to_string(),
                        target: target_node.id.clone(),
                        kind: edge.weight().kind.clone(),
                    });
                }
            }
        }
        Ok(edges)
    }

    pub fn incoming_edges(&self, target: &str) -> Result<Vec<Edge>> {
        let mut edges = Vec::new();
        if let Some(tgt_idx) = self.node_indices.get(target) {
            for edge in self.graph.edges_directed(*tgt_idx, Direction::Incoming) {
                if let Some(source_node) = self.graph.node_weight(edge.source()) {
                    edges.push(Edge {
                        source: source_node.id.clone(),
                        target: target.to_string(),
                        kind: edge.weight().kind.clone(),
                    });
                }
            }
        }
        Ok(edges)
    }

    pub fn list_symbols(&self) -> Result<Vec<GraphNode>> {
        Ok(self.symbol_nodes.iter()
            .filter_map(|&idx| self.graph.node_weight(idx).cloned())
            .collect())
    }

    pub fn list_all_nodes(&self) -> Result<Vec<GraphNode>> {
        Ok(self.graph.node_weights().cloned().collect())
    }
    
    pub fn nodes_matching_label(&self, needle: &str) -> Result<Vec<GraphNode>> {
        let lower = needle.to_lowercase();
        Ok(self.graph.node_weights().filter(|n| {
             n.canonical_id.as_ref().map(|id| id.to_lowercase().contains(&lower)).unwrap_or(false)
             || n.label.to_lowercase().contains(&lower)
        }).cloned().collect())
    }

    pub fn shortest_path(&self, from: &str, to: &str, _max_depth: usize) -> Result<Option<Vec<GraphNode>>> {
        let start = match self.node_indices.get(from) {
            Some(idx) => *idx,
            None => return Ok(None),
        };
        let end = match self.node_indices.get(to) {
            Some(idx) => *idx,
            None => return Ok(None),
        };

        let path_indices = petgraph::algo::astar(
            &self.graph,
            start,
            |finish| finish == end,
            |_| 1,
            |_| 0,
        );

        if let Some((_, indices)) = path_indices {
            let nodes: Vec<GraphNode> = indices.into_iter()
                .filter_map(|idx| self.graph.node_weight(idx).cloned())
                .collect();
            Ok(Some(nodes))
        } else {
            Ok(None)
        }
    }

    pub fn delete_nodes_for_file(&mut self, file_path: &str) -> Result<()> {
        // Use index to find nodes to remove
        let to_remove: Vec<String> = if let Some(indices) = self.files_to_nodes.get(file_path) {
             indices.iter()
                .filter_map(|&idx| self.graph.node_weight(idx).map(|n| n.id.clone()))
                .collect()
        } else {
            Vec::new()
        };
        
        for id in to_remove {
            self.remove_node(&id)?;
        }
        
        // Clean up empty entry in files_to_nodes
        self.files_to_nodes.remove(file_path);
        
        Ok(())
    }

    pub fn resolve_node_id(&self, query: &str, kind: Option<&str>) -> Result<String, ResolutionError> {
        // 1. Exact ID match (fast path)
        if self.node_indices.contains_key(query) {
            // If kind is specified, verify it matches
            if let Some(k) = kind {
                if let Ok(Some(node)) = self.get_node(query) {
                    if node.kind != k {
                        return Err(ResolutionError::NotFound(format!("{} (kind mismatch)", query)));
                    }
                }
            }
            return Ok(query.to_string());
        }

        // 2. Label match (fuzzy/search)
        let matches = self.nodes_matching_label(query)?;
        
        // Filter by kind if specified
        let filtered_matches: Vec<GraphNode> = if let Some(k) = kind {
            matches.into_iter().filter(|n| n.kind == k).collect()
        } else {
            matches
        };

        if filtered_matches.is_empty() {
            return Err(ResolutionError::NotFound(query.to_string()));
        }
        if filtered_matches.len() == 1 {
            return Ok(filtered_matches[0].id.clone());
        }

        // Ambiguous
        // Smart Default: If kind is NOT specified, apply tie-breaking rules.
        // Priority: File > Symbol > Chunk
        if kind.is_none() {
            // Check for File
            if let Some(f) = filtered_matches.iter().find(|n| n.kind == "file") {
                return Ok(f.id.clone());
            }
            // Check for Symbol
            if let Some(s) = filtered_matches.iter().find(|n| n.kind == "symbol") {
                return Ok(s.id.clone());
            }
            // Check for Chunk (unlikely to be desired via fuzzy search, but fallback)
            if let Some(c) = filtered_matches.iter().find(|n| n.kind == "chunk") {
                return Ok(c.id.clone());
            }
        }

        let candidates = filtered_matches.iter().map(|n| n.id.clone()).collect();
        Err(ResolutionError::Ambiguous(query.to_string(), candidates))
    }
}

#[derive(Debug)]
pub enum ResolutionError {
    NotFound(String),
    Ambiguous(String, Vec<String>),
    GraphError(anyhow::Error),
}

impl std::fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolutionError::NotFound(q) => write!(f, "Node not found: {}", q),
            ResolutionError::Ambiguous(q, c) => write!(f, "Ambiguous node reference '{}'. Candidates: {:?}", q, c),
            ResolutionError::GraphError(e) => write!(f, "Graph error: {}", e),
        }
    }
}

impl std::error::Error for ResolutionError {}

impl From<anyhow::Error> for ResolutionError {
    fn from(e: anyhow::Error) -> Self {
        ResolutionError::GraphError(e)
    }
}