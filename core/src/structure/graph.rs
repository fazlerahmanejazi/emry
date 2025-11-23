use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use tree_sitter::{Parser, Query, QueryCursor};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeType {
    Function,
    Class,
    File,
    Module,
    Variable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeType,
    pub label: String, // Name or path
    #[serde(default)]
    pub fqn: Option<String>,
    #[serde(default)]
    pub language: Option<crate::models::Language>,
    pub file_path: Option<PathBuf>,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default)]
    pub chunk_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeType {
    Calls,
    DefinedIn,
    Imports,
    MemberOf,
    DataFlow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    pub kind: EdgeType,
    #[serde(default)]
    pub confidence: f32,
}

use crate::storage::NodeStorage;
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CodeGraph {
    pub nodes: HashMap<NodeId, GraphNode>,
    pub edges: Vec<Edge>,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    pub storage: Option<Arc<dyn NodeStorage>>,
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
            storage: None,
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

    pub fn with_storage(mut self, storage: Arc<dyn NodeStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn get_node(&self, id: &NodeId) -> Option<GraphNode> {
        if let Some(node) = self.nodes.get(id) {
            return Some(node.clone());
        }
        if let Some(storage) = &self.storage {
            if let Ok(Some(node)) = storage.get(id) {
                return Some(node);
            }
        }
        None
    }

    pub fn add_node(
        &mut self,
        id: NodeId,
        kind: NodeType,
        label: String,
        fqn: Option<String>,
        language: Option<crate::models::Language>,
        file_path: Option<PathBuf>,
        start_line: usize,
        end_line: usize,
        chunk_ids: Vec<String>,
    ) {
        let node = GraphNode {
            id: id.clone(),
            kind,
            label,
            fqn,
            language,
            file_path,
            start_line,
            end_line,
            chunk_ids,
        };

        if let Some(storage) = &self.storage {
            let _ = storage.insert(&id, &node);
            // If using storage, we might choose NOT to store in RAM to save space.
            // For now, let's keep it in RAM too for safety until we fully migrate.
             self.nodes.insert(id, node);
        } else {
            self.nodes.insert(id, node);
        }
    }

    pub fn add_edge(&mut self, source: NodeId, target: NodeId, kind: EdgeType) {
        self.edges.push(Edge {
            source,
            target,
            kind,
            confidence: 1.0,
        });
    }

    pub fn find_node_for_chunk(&self, chunk: &crate::models::Chunk) -> Option<NodeId> {
        let mut best: Option<(NodeId, usize)> = None;
        for (id, node) in &self.nodes {
            if let Some(path) = &node.file_path {
                if path.to_string_lossy() != chunk.file_path.to_string_lossy() {
                    continue;
                }
                let overlap = overlap_lines(
                    node.start_line,
                    node.end_line,
                    chunk.start_line,
                    chunk.end_line,
                );
                if overlap == 0 {
                    continue;
                }
                let bonus = match node.kind {
                    NodeType::Function | NodeType::Module => 1000,
                    NodeType::Class => 500,
                    _ => 0,
                };
                let score = overlap + bonus;
                if best.as_ref().map(|b| score > b.1).unwrap_or(true) {
                    best = Some((id.clone(), score));
                }
            }
        }
        best.map(|b| b.0)
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }

    pub fn as_petgraph(
        &self,
    ) -> (
        petgraph::Graph<NodeId, EdgeType>,
        HashMap<NodeId, petgraph::graph::NodeIndex>,
    ) {
        let mut graph = petgraph::Graph::new();
        let mut node_map = HashMap::new();

        // Add nodes
        for (id, _) in &self.nodes {
            let idx = graph.add_node(id.clone());
            node_map.insert(id.clone(), idx);
        }

        // Add edges
        for edge in &self.edges {
            if let (Some(&source_idx), Some(&target_idx)) =
                (node_map.get(&edge.source), node_map.get(&edge.target))
            {
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
                Some(symbol.fqn.clone()),
                Some(symbol.language.clone()),
                Some(symbol.file_path.clone()),
                symbol.start_line,
                symbol.end_line,
                symbol.chunk_ids.clone(),
            );

            // Add node for the file if it doesn't exist
            let file_id = NodeId(symbol.file_path.to_string_lossy().to_string());
            if !graph.nodes.contains_key(&file_id) {
                graph.add_node(
                    file_id.clone(),
                    NodeType::File,
                    symbol
                        .file_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    None,
                    Some(symbol.language.clone()),
                    Some(symbol.file_path.clone()),
                    0,
                    0,
                    Vec::new(),
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
                    if let Some(container) = classes.iter().find(|c| {
                        c.start_line <= method.start_line && c.end_line >= method.end_line
                    }) {
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
            if !sym.fqn.is_empty() {
                symbol_map.entry(sym.fqn.clone()).or_default().push(sym);
            }
        }

        // Map file -> symbols in that file (sorted by start)
        let mut file_symbols: HashMap<PathBuf, Vec<&Symbol>> = HashMap::new();
        for sym in symbols {
            file_symbols
                .entry(sym.file_path.clone())
                .or_default()
                .push(sym);
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
            let imports = extract_import_refs(content, language);
            let alias_map: HashMap<String, String> = imports
                .iter()
                .filter_map(|imp| imp.alias.clone().map(|a| (a, imp.module.clone())))
                .collect();

            // Calls: connect caller symbol -> callee symbol
            for call in calls {
                // Find caller symbol by line
                let caller_sym = file_symbols
                    .get(path)
                    .and_then(|syms| {
                        syms.iter()
                            .find(|s| s.start_line <= call.line && s.end_line >= call.line)
                    })
                    .cloned();

                if let Some(caller) = caller_sym {
                    let mut candidate_callees = Vec::new();
                    if let Some(callees) = symbol_map.get(&call.name) {
                        candidate_callees.extend_from_slice(callees);
                    }
                    if let Some(qual) = &call.qualifier {
                        // Try qualifier + name as FQN-ish lookup
                        let qual_name = format!("{}::{}", qual, call.name);
                        if let Some(callees) = symbol_map.get(&qual_name) {
                            candidate_callees.extend_from_slice(callees);
                        }
                        if let Some(mod_path) = alias_map.get(qual) {
                            let mod_pref = mod_path.replace('.', "::");
                            for sym in symbols {
                                if sym.fqn.starts_with(&mod_pref) && sym.name == call.name {
                                    candidate_callees.push(sym);
                                }
                            }
                        }
                    }
                    // module_hint path (TypeScript/Java dotted)
                    if let Some(mod_hint) = &call.module_hint {
                        let mod_fqn = format!("{}::{}", mod_hint.replace('.', "::"), call.name);
                        if let Some(callees) = symbol_map.get(&mod_fqn) {
                            candidate_callees.extend_from_slice(callees);
                        }
                    }

                    // Prefer same file/module matches
                    if let Some(target) = candidate_callees
                        .iter()
                        .find(|c| c.file_path == caller.file_path)
                        .copied()
                    {
                        let edge = (
                            NodeId(caller.id.clone()),
                            NodeId(target.id.clone()),
                            EdgeType::Calls,
                        );
                        if existing_edges.insert(edge.clone()) {
                            graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                        }
                    } else {
                        for target in candidate_callees {
                            let edge = (
                                NodeId(caller.id.clone()),
                                NodeId(target.id.clone()),
                                EdgeType::Calls,
                            );
                            if existing_edges.insert(edge.clone()) {
                                graph.edges.push(Edge {
                                    source: edge.0.clone(),
                                    target: edge.1.clone(),
                                    kind: EdgeType::Calls,
                                    confidence: 0.5,
                                });
                            }
                        }
                    }
                }
            }

            // Imports: connect file -> imported file if name matches
            for imp in imports {
                // Match by module path stem
                if let Some(target_id) =
                    file_name_map.get(imp.module.split('.').last().unwrap_or(&imp.module))
                {
                    let edge = (caller_file_id.clone(), target_id.clone(), EdgeType::Imports);
                    if existing_edges.insert(edge.clone()) {
                        graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                    }
                }
                // Match alias to symbols
                if let Some(alias) = &imp.alias {
                    if let Some(targets) = symbol_map.get(alias) {
                        for sym in targets {
                            let edge = (
                                caller_file_id.clone(),
                                NodeId(sym.id.clone()),
                                EdgeType::Imports,
                            );
                            if existing_edges.insert(edge.clone()) {
                                graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                            }
                        }
                    }
                }
            }
        }

        build_data_flow_edges(graph, &file_symbols, files, &mut existing_edges);
    }

    /// Remove nodes and edges belonging to the specified files (by path match).
    pub fn prune_files(graph: &mut CodeGraph, files: &HashSet<PathBuf>) {
        graph.nodes.retain(|_, node| {
            if let Some(fp) = &node.file_path {
                !files.contains(fp)
            } else {
                true
            }
        });
        graph.edges.retain(|edge| {
            let keep_src = graph.nodes.contains_key(&edge.source);
            let keep_dst = graph.nodes.contains_key(&edge.target);
            keep_src && keep_dst
        });
    }
}

fn build_data_flow_edges(
    graph: &mut CodeGraph,
    file_symbols: &HashMap<PathBuf, Vec<&Symbol>>,
    files: &[(PathBuf, crate::models::Language, String)],
    existing_edges: &mut HashSet<(NodeId, NodeId, EdgeType)>,
) {
    for (path, language, content) in files {
        let syms_in_file = match file_symbols.get(path) {
            Some(s) => s,
            None => continue,
        };
        let lines: Vec<&str> = content.lines().collect();
        for func in syms_in_file {
            if !matches!(func.kind, SymbolKind::Function | SymbolKind::Method) {
                continue;
            }
            let start = func.start_line.saturating_sub(1);
            let end = func.end_line.min(lines.len());
            let mut defs: Vec<(String, usize)> = Vec::new();
            let mut uses: Vec<(String, usize)> = Vec::new();
            for (idx, line) in lines[start..end].iter().enumerate() {
                let line_no = start + idx + 1;
                let (mut d, mut u) = tokenize_defs_and_uses(line, language);
                defs.extend(d.drain(..).map(|n| (n, line_no)));
                uses.extend(u.drain(..).map(|n| (n, line_no)));
            }
            for (name, line_no) in defs {
                let var_id = NodeId(format!(
                    "{}:{}:{}:var",
                    path.to_string_lossy(),
                    name,
                    line_no
                ));
                if !graph.nodes.contains_key(&var_id) {
                    graph.add_node(
                        var_id.clone(),
                        NodeType::Variable,
                        name.clone(),
                        None,
                        Some(func.language.clone()),
                        Some(path.clone()),
                        line_no,
                        line_no,
                        Vec::new(),
                    );
                }
                let func_node = NodeId(func.id.clone());
                // variable -> function (data flow)
                let edge = (var_id.clone(), func_node.clone(), EdgeType::DataFlow);
                if existing_edges.insert(edge.clone()) {
                    graph.add_edge(edge.0.clone(), edge.1.clone(), edge.2.clone());
                }
                // variable defined in file
                let file_id = NodeId(path.to_string_lossy().to_string());
                let def_edge = (var_id.clone(), file_id.clone(), EdgeType::DefinedIn);
                if existing_edges.insert(def_edge.clone()) {
                    graph.add_edge(def_edge.0.clone(), def_edge.1.clone(), def_edge.2.clone());
                }
                // variable member of function
                let mem_edge = (var_id.clone(), func_node.clone(), EdgeType::MemberOf);
                if existing_edges.insert(mem_edge.clone()) {
                    graph.add_edge(mem_edge.0.clone(), mem_edge.1.clone(), mem_edge.2.clone());
                }

                for (u_name, u_line) in uses.iter().filter(|(n, _)| n == &name) {
                    let use_node = NodeId(format!(
                        "{}:{}:{}:use",
                        path.to_string_lossy(),
                        u_name,
                        u_line
                    ));
                    if !graph.nodes.contains_key(&use_node) {
                        graph.add_node(
                            use_node.clone(),
                            NodeType::Variable,
                            u_name.clone(),
                            None,
                            Some(func.language.clone()),
                            Some(path.clone()),
                            *u_line,
                            *u_line,
                            Vec::new(),
                        );
                    }
                    let flow_edge = (var_id.clone(), use_node.clone(), EdgeType::DataFlow);
                    if existing_edges.insert(flow_edge.clone()) {
                        graph.add_edge(
                            flow_edge.0.clone(),
                            flow_edge.1.clone(),
                            flow_edge.2.clone(),
                        );
                    }
                    let use_mem = (use_node.clone(), func_node.clone(), EdgeType::MemberOf);
                    if existing_edges.insert(use_mem.clone()) {
                        graph.add_edge(use_mem.0.clone(), use_mem.1.clone(), use_mem.2.clone());
                    }
                }
            }
        }
    }
}

fn tokenize_defs_and_uses(
    line: &str,
    language: &crate::models::Language,
) -> (Vec<String>, Vec<String>) {
    let mut defs = Vec::new();
    let mut uses = Vec::new();

    match language {
        crate::models::Language::Python => {
            if let Some(eq_idx) = line.find('=') {
                let lhs = line[..eq_idx].trim();
                if !lhs.is_empty() {
                    let ident = lhs
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .last()
                        .unwrap_or(lhs)
                        .to_string();
                    defs.push(ident);
                }
            }
        }
        _ => {
            let decl_keys = [
                "let", "const", "var", "mut", "int", "float", "double", "char", "auto",
            ];
            if decl_keys.iter().any(|k| line.contains(k)) {
                let tokens: Vec<&str> = line
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .filter(|s| !s.is_empty())
                    .collect();
                if tokens.len() >= 2 {
                    defs.push(tokens[1].to_string());
                }
            }
        }
    }

    let tokens: Vec<&str> = line
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
        .filter(|s| !s.is_empty())
        .collect();
    uses.extend(tokens.into_iter().map(|s| s.to_string()));

    (defs, uses)
}
fn extract_import_refs(content: &str, language: &crate::models::Language) -> Vec<ImportRef> {
    match language {
        crate::models::Language::Python => content
            .lines()
            .filter_map(|l| {
                let trimmed = l.trim_start();
                if trimmed.starts_with("import ") {
                    let mut parts = trimmed.split_whitespace();
                    parts.next();
                    parts.next().map(|s| ImportRef {
                        module: s.to_string(),
                        alias: None,
                    })
                } else if trimmed.starts_with("from ") {
                    let mut parts = trimmed.split_whitespace();
                    parts.next();
                    let module = parts.next();
                    if let Some(m) = module {
                        let alias = parts.nth(1).map(|s| s.trim_matches(',').to_string());
                        return Some(ImportRef {
                            module: m.to_string(),
                            alias,
                        });
                    }
                    None
                } else {
                    None
                }
            })
            .collect(),
        crate::models::Language::TypeScript | crate::models::Language::JavaScript => {
            let mut imports: Vec<ImportRef> = Vec::new();
            for l in content.lines() {
                let trimmed = l.trim_start();
                if trimmed.starts_with("import ") {
                    if let Some(idx) = trimmed.find("from") {
                        if let Some(rest) = trimmed.get(idx + 4..) {
                            let mod_str = rest
                                .trim()
                                .trim_matches(&['"', '\'', ';', ' ', '{', '}'][..]);
                            let stem = mod_str.split('/').last().unwrap_or(mod_str);
                            let default_alias = trimmed
                                .split_whitespace()
                                .nth(1)
                                .map(|s| s.trim_matches(&['{', '}', ','][..]).to_string());
                            imports.push(ImportRef {
                                module: stem.trim_matches(&['"', '\''][..]).to_string(),
                                alias: default_alias,
                            });
                        }
                    } else if let Some(name) = trimmed.split_whitespace().nth(1) {
                        imports.push(ImportRef {
                            module: name.trim_matches(&[';', ' '][..]).to_string(),
                            alias: None,
                        });
                    }
                } else if trimmed.contains("require(") {
                    if let Some(start) = trimmed.find("require(") {
                        if let Some(rest) = trimmed.get(start + 8..) {
                            let mod_str = rest.trim().trim_matches(&['"', '\'', ')', ';'][..]);
                            if let Some(stem) = mod_str.split('/').last() {
                                imports.push(ImportRef {
                                    module: stem.to_string(),
                                    alias: None,
                                });
                            }
                        }
                    }
                }
            }
            imports
        }
        crate::models::Language::Java => content
            .lines()
            .filter_map(|l| {
                let trimmed = l.trim_start();
                if trimmed.starts_with("import ") {
                    return trimmed.split_whitespace().nth(1).map(|s| ImportRef {
                        module: s.trim_end_matches(';').to_string(),
                        alias: None,
                    });
                }
                None
            })
            .collect(),
        crate::models::Language::Cpp => content
            .lines()
            .filter_map(|l| {
                let trimmed = l.trim_start();
                if trimmed.starts_with("#include") {
                    return trimmed.split_whitespace().nth(1).map(|s| ImportRef {
                        module: s.trim_matches(&['<', '>', '"'][..]).to_string(),
                        alias: None,
                    });
                }
                None
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn overlap_lines(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> usize {
    let start = std::cmp::max(a_start, b_start);
    let end = std::cmp::min(a_end, b_end);
    if start > end {
        0
    } else {
        end - start + 1
    }
}

struct CallHit {
    name: String,
    line: usize,
    qualifier: Option<String>,
    module_hint: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportRef {
    module: String,
    alias: Option<String>,
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
                                             (call function: (attribute object: (identifier) @qualifier attribute: (identifier) @name)) @call
                                             (call function: (attribute object: (attribute object: (identifier) @qualifier) attribute: (identifier) @name)) @call",
        crate::models::Language::TypeScript | crate::models::Language::JavaScript => "(call_expression function: (identifier) @name)
                                                                                      (call_expression function: (member_expression object: (identifier) @qualifier property: (property_identifier) @name))
                                                                                      (call_expression function: (member_expression property: (property_identifier) @name))",
        crate::models::Language::Java => "(method_invocation object: (identifier) @qualifier name: (identifier) @name)
                                          (method_invocation name: (identifier) @name)",
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
        let mut name_val: Option<String> = None;
        let mut qual_val: Option<String> = None;
        for cap in m.captures {
            if query.capture_names()[cap.index as usize] == "name" {
                if let Ok(name) = cap.node.utf8_text(content.as_bytes()) {
                    name_val = Some(name.to_string());
                }
            }
            if query.capture_names()[cap.index as usize] == "qualifier" {
                if let Ok(q) = cap.node.utf8_text(content.as_bytes()) {
                    qual_val = Some(q.to_string());
                }
            }
        }
        if let Some(name) = name_val {
            hits.push(CallHit {
                name,
                line: m
                    .captures
                    .first()
                    .map(|c| c.node.start_position().row + 1)
                    .unwrap_or(0),
                qualifier: qual_val.clone(),
                module_hint: qual_val,
            });
        }
    }
    hits
}
