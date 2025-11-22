use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeType {
    Function,
    Class,
    File,
    Module,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeType,
    pub label: String, // Name or path
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Calls,
    DefinedIn,
    Imports,
    MemberOf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    pub kind: EdgeType,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CodeGraph {
    pub nodes: HashMap<NodeId, GraphNode>,
    pub edges: Vec<Edge>,
    #[serde(skip)]
    path: PathBuf,
}

impl CodeGraph {
    pub fn new(path: &Path) -> Self {
        if path.exists() {
            if let Ok(graph) = Self::load(path) {
                return graph;
            }
        }
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            path: path.to_path_buf(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut graph: CodeGraph = serde_json::from_reader(reader)?;
        graph.path = path.to_path_buf();
        Ok(graph)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(&self.path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, &self)?;
        Ok(())
    }

    pub fn add_node(&mut self, id: NodeId, kind: NodeType, label: String, file_path: Option<PathBuf>) {
        self.nodes.insert(id.clone(), GraphNode {
            id,
            kind,
            label,
            file_path,
        });
    }

    pub fn add_edge(&mut self, source: NodeId, target: NodeId, kind: EdgeType) {
        self.edges.push(Edge {
            source,
            target,
            kind,
        });
    }
    
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }
}

use crate::structure::symbols::{Symbol, SymbolKind};

pub struct GraphBuilder;

impl GraphBuilder {
    pub fn build(graph: &mut CodeGraph, symbols: &[Symbol]) {
        for symbol in symbols {
            // Add node for the symbol
            let node_id = NodeId(symbol.id.clone());
            let kind = match symbol.kind {
                SymbolKind::Function => NodeType::Function,
                SymbolKind::Method => NodeType::Function, // Treat methods as functions for now or add Method type
                SymbolKind::Class => NodeType::Class,
                SymbolKind::Interface => NodeType::Class,
                SymbolKind::Module => NodeType::Module,
                SymbolKind::Unknown => NodeType::Function,
            };
            
            graph.add_node(
                node_id.clone(),
                kind,
                symbol.name.clone(),
                Some(symbol.file_path.clone()),
            );

            // Add node for the file if it doesn't exist
            let file_id = NodeId(symbol.file_path.to_string_lossy().to_string());
            if !graph.nodes.contains_key(&file_id) {
                graph.add_node(
                    file_id.clone(),
                    NodeType::File,
                    symbol.file_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                    Some(symbol.file_path.clone()),
                );
            }

            // Edge: Symbol DefinedIn File
            graph.add_edge(node_id.clone(), file_id.clone(), EdgeType::DefinedIn);

            // Edge: MemberOf (Method -> Class)
            // This is tricky without parent info in Symbol. 
            // For Phase 2, we can try to infer from ID if we structured it hierarchically, 
            // or we need to update SymbolExtractor to provide parent ID.
            // For now, we'll skip MemberOf unless we parse it.
            // Actually, let's leave it for now and focus on DefinedIn.
        }
    }
}
