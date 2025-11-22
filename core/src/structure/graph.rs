use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use anyhow::Result;
use tree_sitter::{Parser, Query, QueryCursor};

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
    pub start_line: usize,
    pub end_line: usize,
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

    pub fn add_node(&mut self, id: NodeId, kind: NodeType, label: String, file_path: Option<PathBuf>, start_line: usize, end_line: usize) {
        self.nodes.insert(id.clone(), GraphNode {
            id,
            kind,
            label,
            file_path,
            start_line,
            end_line,
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

    pub fn as_petgraph(&self) -> (petgraph::Graph<NodeId, EdgeType>, HashMap<NodeId, petgraph::graph::NodeIndex>) {
        let mut graph = petgraph::Graph::new();
        let mut node_map = HashMap::new();

        // Add nodes
        for (id, _) in &self.nodes {
            let idx = graph.add_node(id.clone());
            node_map.insert(id.clone(), idx);
        }

        // Add edges
        for edge in &self.edges {
            if let (Some(&source_idx), Some(&target_idx)) = (node_map.get(&edge.source), node_map.get(&edge.target)) {
                graph.add_edge(source_idx, target_idx, edge.kind.clone());
            }
        }

        (graph, node_map)
    }
}

use crate::structure::symbols::{Symbol, SymbolKind};

pub struct GraphBuilder;

impl GraphBuilder {
    pub fn build(graph: &mut CodeGraph, symbols: &[Symbol]) {
        let mut existing_edges: HashSet<(NodeId, NodeId, EdgeType)> = graph
            .edges
            .iter()
            .map(|e| (e.source.clone(), e.target.clone(), e.kind.clone()))
            .collect();

        let mut classes_by_file: HashMap<PathBuf, Vec<&Symbol>> = HashMap::new();
        let mut methods_by_file: HashMap<PathBuf, Vec<&Symbol>> = HashMap::new();

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
                symbol.start_line,
                symbol.end_line,
            );

            // Add node for the file if it doesn't exist
            let file_id = NodeId(symbol.file_path.to_string_lossy().to_string());
            if !graph.nodes.contains_key(&file_id) {
                graph.add_node(
                    file_id.clone(),
                    NodeType::File,
                    symbol.file_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                    Some(symbol.file_path.clone()),
                    0,
                    0,
                );
            }

            // Edge: Symbol DefinedIn File
            let forward = (node_id.clone(), file_id.clone(), EdgeType::DefinedIn);
            if existing_edges.insert(forward.clone()) {
                graph.add_edge(forward.0.clone(), forward.1.clone(), forward.2.clone());
            }
            let backward = (file_id.clone(), node_id.clone(), EdgeType::DefinedIn);
            if existing_edges.insert(backward.clone()) {
                graph.add_edge(backward.0.clone(), backward.1.clone(), backward.2.clone());
            }

            // Edge: MemberOf (Method -> Class)
            match symbol.kind {
                SymbolKind::Class | SymbolKind::Interface => {
                    classes_by_file
                        .entry(symbol.file_path.clone())
                        .or_default()
                        .push(symbol);
                }
                SymbolKind::Method => {
                    methods_by_file
                        .entry(symbol.file_path.clone())
                        .or_default()
                        .push(symbol);
                }
                _ => {}
            }
        }

        // Add MemberOf edges by range containment (method inside class in same file)
        for (file, methods) in methods_by_file {
            if let Some(classes) = classes_by_file.get(&file) {
                for method in methods {
                    if let Some(container) = classes
                        .iter()
                        .find(|c| c.start_line <= method.start_line && c.end_line >= method.end_line)
                    {
                        let edge = (
                            NodeId(method.id.clone()),
                            NodeId(container.id.clone()),
                            EdgeType::MemberOf,
                        );
                        if existing_edges.insert(edge.clone()) {
                            graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                        }
                    }
                }
            }
        }
    }

    pub fn build_calls_and_imports(
        graph: &mut CodeGraph,
        symbols: &[Symbol],
        files: &[(PathBuf, crate::models::Language, String)],
    ) {
        // Map symbol name -> symbols
        let mut symbol_map: HashMap<String, Vec<&Symbol>> = HashMap::new();
        for sym in symbols {
            symbol_map.entry(sym.name.clone()).or_default().push(sym);
        }

        // Map file -> symbols in that file (sorted by start)
        let mut file_symbols: HashMap<PathBuf, Vec<&Symbol>> = HashMap::new();
        for sym in symbols {
            file_symbols.entry(sym.file_path.clone()).or_default().push(sym);
        }
        for syms in file_symbols.values_mut() {
            syms.sort_by_key(|s| s.start_line);
        }

        // Map file base name -> file NodeId for naive import resolution
        let mut file_name_map: HashMap<String, NodeId> = HashMap::new();
        for node in graph.nodes.values() {
            if node.kind == NodeType::File {
                if let Some(path) = &node.file_path {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        file_name_map.insert(stem.to_string(), node.id.clone());
                    }
                }
            }
        }

        let mut existing_edges: HashSet<(NodeId, NodeId, EdgeType)> = graph
            .edges
            .iter()
            .map(|e| (e.source.clone(), e.target.clone(), e.kind.clone()))
            .collect();

        for (path, language, content) in files {
            let caller_file_id = NodeId(path.to_string_lossy().to_string());
            let calls = extract_call_names(content, language);
            let imports = extract_import_names(content, language);

            // Calls: connect caller symbol -> callee symbol
            for call in calls {
                // Find caller symbol by line
                let caller_sym = file_symbols
                    .get(path)
                    .and_then(|syms| syms.iter().find(|s| s.start_line <= call.line && s.end_line >= call.line))
                    .cloned();

                let callee_sym = symbol_map.get(&call.name).and_then(|v| v.first()).cloned();

                if let (Some(caller), Some(callee)) = (caller_sym, callee_sym) {
                    let edge = (
                        NodeId(caller.id.clone()),
                        NodeId(callee.id.clone()),
                        EdgeType::Calls,
                    );
                    if existing_edges.insert(edge.clone()) {
                        graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                    }
                }
            }

            // Imports: connect file -> imported file if name matches
            for imp in imports {
                if let Some(target_id) = file_name_map.get(&imp) {
                    let edge = (caller_file_id.clone(), target_id.clone(), EdgeType::Imports);
                    if existing_edges.insert(edge.clone()) {
                        graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                    }
                }
            }
        }
    }
}

struct CallHit {
    name: String,
    line: usize,
}

fn extract_call_names(content: &str, language: &crate::models::Language) -> Vec<CallHit> {
    let mut parser = Parser::new();
    let lang = match language {
        crate::models::Language::Python => tree_sitter_python::LANGUAGE.into(),
        crate::models::Language::TypeScript | crate::models::Language::JavaScript => {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        }
        crate::models::Language::Java => tree_sitter_java::LANGUAGE.into(),
        crate::models::Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        _ => return Vec::new(),
    };
    if parser.set_language(&lang).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let query_str = match language {
        crate::models::Language::Python => "(call function: (identifier) @name) @call
                                             (call function: (attribute attribute: (identifier) @name)) @call",
        crate::models::Language::TypeScript | crate::models::Language::JavaScript => "(call_expression function: (identifier) @name)
                                                                                      (call_expression function: (member_expression property: (property_identifier) @name))",
        crate::models::Language::Java => "(method_invocation name: (identifier) @name)",
        crate::models::Language::Cpp => "(call_expression function: (identifier) @name)",
        _ => "",
    };
    let query = Query::new(&lang, query_str);
    let query = match query {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };

    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
    let mut hits = Vec::new();
    for m in matches {
        for cap in m.captures {
            if query.capture_names()[cap.index as usize] == "name" {
                if let Ok(name) = cap.node.utf8_text(content.as_bytes()) {
                    hits.push(CallHit {
                        name: name.to_string(),
                        line: cap.node.start_position().row + 1,
                    });
                }
            }
        }
    }
    hits
}

fn extract_import_names(content: &str, language: &crate::models::Language) -> Vec<String> {
    match language {
        crate::models::Language::Python => {
            // Very simple: capture words after "import" or "from X import"
            content
                .lines()
                .filter_map(|l| {
                    let trimmed = l.trim_start();
                    if trimmed.starts_with("import ") {
                        trimmed
                            .split_whitespace()
                            .nth(1)
                            .map(|s| s.split('.').next().unwrap_or(s).to_string())
                    } else if trimmed.starts_with("from ") {
                        trimmed
                            .split_whitespace()
                            .nth(1)
                            .map(|s| s.split('.').next().unwrap_or(s).to_string())
                    } else {
                        None
                    }
                })
                .collect()
        }
        crate::models::Language::TypeScript | crate::models::Language::JavaScript => {
            // Look for: import X from '...'; or require("x")
            let mut imports = Vec::new();
            for l in content.lines() {
                let trimmed = l.trim_start();
                if trimmed.starts_with("import ") {
                    // import {A} from './foo';
                    if let Some(idx) = trimmed.find("from") {
                        if let Some(rest) = trimmed.get(idx + 4..) {
                            let mod_str = rest.trim().trim_matches(&['"', '\'', ';', ' ', '{', '}'][..]);
                            if let Some(stem) = mod_str.split('/').last() {
                                imports.push(stem.trim_matches(&['"', '\''][..]).to_string());
                            }
                        }
                    } else {
                        // import Foo;
                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        if parts.len() >= 2 {
                            imports.push(parts[1].trim_matches(&[';', '{', '}', ' '][..]).to_string());
                        }
                    }
                } else if trimmed.contains("require(") {
                    if let Some(start) = trimmed.find("require(") {
                        if let Some(rest) = trimmed.get(start + 8..) {
                            let mod_str = rest.trim().trim_matches(&['"', '\'', ')', ';'][..]);
                            if let Some(stem) = mod_str.split('/').last() {
                                imports.push(stem.to_string());
                            }
                        }
                    }
                }
            }
            imports
        }
        crate::models::Language::Java => {
            content
                .lines()
                .filter_map(|l| {
                    let trimmed = l.trim_start();
                    if trimmed.starts_with("import ") {
                        trimmed
                            .strip_prefix("import ")
                            .and_then(|s| s.split_whitespace().next())
                            .map(|s| s.split('.').last().unwrap_or(s).trim_end_matches(';').to_string())
                    } else {
                        None
                    }
                })
                .collect()
        }
        crate::models::Language::Cpp => {
            content
                .lines()
                .filter_map(|l| {
                    let trimmed = l.trim_start();
                    if trimmed.starts_with("#include") {
                        trimmed
                            .split_whitespace()
                            .nth(1)
                            .map(|s| s.trim_matches(&['<', '>', '"'][..]).to_string())
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}
