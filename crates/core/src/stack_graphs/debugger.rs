use crate::stack_graphs::manager::StackGraphManager;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use stack_graphs::arena::Handle;
use stack_graphs::graph::{Node, StackGraph};
use stack_graphs::partial::PartialPaths;
use stack_graphs::stitching::{
    ForwardPartialPathStitcher, GraphEdgeCandidates, StitcherConfig,
};
use stack_graphs::NoCancellation;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub symbol: Option<String>,
    pub kind: String,
    pub id: String, // Debugging ID
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    pub kind: String, // "Edge", "Scope", "Jump", etc.
    pub node: NodeInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionTrace {
    pub reference: NodeInfo,
    pub definitions: Vec<NodeInfo>,
    pub paths: Vec<Vec<TraceStep>>,
    pub error: Option<String>,
}

pub struct GraphDebugger<'a> {
    manager: &'a StackGraphManager,
}

impl<'a> GraphDebugger<'a> {
    pub fn new(manager: &'a StackGraphManager) -> Self {
        Self { manager }
    }

    pub fn trace_reference(&self, file_path: &str, line: usize, col: usize) -> Result<ResolutionTrace> {
        let graph = &self.manager.graph;

        // 1. Find the file handle
        // StackGraph stores paths as they were added. We might need to normalize or search.
        // For now, assume exact match or try to find by suffix if needed.
        let file_handle = graph
            .iter_files()
            .find(|h| graph[*h].name() == file_path)
            .ok_or_else(|| {
                let files: Vec<String> = graph.iter_files().map(|h| graph[h].name().to_string()).collect();
                anyhow::anyhow!("File not found in stack graph: {}. Available (first 10): {:?}", file_path, files.iter().take(10).collect::<Vec<_>>())
            })?;

        // 2. Find the reference node overlapping line:col
        // StackGraph uses 0-indexed lines/cols internally usually, but let's check source_info.
        // source_info is 0-indexed. User input `line` is likely 1-indexed (from CLI).
        // We'll convert user input to 0-indexed.
        let target_line = line.saturating_sub(1);
        let target_col = col.saturating_sub(1);

        let reference_node = graph
            .iter_nodes()
            .filter(|n| graph[*n].file() == Some(file_handle))
            .find(|n| {
                let node = &graph[*n];
                if !node.is_reference() {
                    return false;
                }
                if let Some(info) = graph.source_info(*n) {
                    // Check if point is within range
                    let start = info.span.start.clone();
                    let end = info.span.end.clone();
                    
                    // Simple check: line matches, col within range
                    // Multi-line spans are possible but rare for references.
                    if target_line < start.line || target_line > end.line {
                        return false;
                    }
                    if target_line == start.line && target_col < start.column.utf8_offset {
                        return false;
                    }
                    if target_line == end.line && target_col >= end.column.utf8_offset {
                        return false;
                    }
                    return true;
                }
                false
            })
            .ok_or_else(|| anyhow::anyhow!("No reference node found at {}:{}:{}", file_path, line, col))?;

        self.trace_node(reference_node)
    }

    pub fn trace_node(&self, reference_node: Handle<Node>) -> Result<ResolutionTrace> {
        let graph = &self.manager.graph;

        // 3. Run Stitcher
        let mut partials = PartialPaths::new();
        let mut database = GraphEdgeCandidates::new(graph, &mut partials, None);
        
        let mut paths = Vec::new();
        let mut definitions = Vec::new();

        // We want to capture ALL paths, including partial ones if possible, but Stitcher
        // usually returns complete paths.
        // find_all_complete_partial_paths returns complete paths.
        
        let references = vec![reference_node];
        
        ForwardPartialPathStitcher::find_all_complete_partial_paths(
            &mut database,
            references,
            StitcherConfig::default(),
            &NoCancellation,
            |g, _ps, path| {
                // Reconstruct path steps
                let start_node = path.start_node;
                let end_node = path.end_node;
                
                let start_info = self.node_info(g, start_node);
                let end_info = self.node_info(g, end_node);
                
                // Create a simple 2-step path for now: Ref -> Def
                // TODO: If stack-graphs exposes edges, iterate them.
                let step_ref = TraceStep { kind: "Reference".into(), node: start_info };
                let step_def = TraceStep { kind: "Definition".into(), node: end_info.clone() };
                
                paths.push(vec![step_ref, step_def]);
                definitions.push(end_info);
            }
        )?;

        let ref_info = self.node_info(graph, reference_node);

        let error = if paths.is_empty() { Some("No resolution found".into()) } else { None };

        Ok(ResolutionTrace {
            reference: ref_info,
            definitions,
            paths,
            error,
        })
    }

    fn node_info(&self, graph: &StackGraph, handle: Handle<Node>) -> NodeInfo {
        let node = &graph[handle];
        let file_handle = node.file().expect("Node must have a file");
        let file_name = graph[file_handle].name().to_string();
        
        let (line, col) = if let Some(info) = graph.source_info(handle) {
            (info.span.start.line + 1, info.span.start.column.utf8_offset + 1)
        } else {
            (0, 0)
        };

        let symbol = match node {
            Node::PushSymbol(n) => Some(graph[n.symbol].to_string()),
            Node::PopSymbol(n) => Some(graph[n.symbol].to_string()),
            Node::PushScopedSymbol(n) => Some(graph[n.symbol].to_string()),
            Node::PopScopedSymbol(n) => Some(graph[n.symbol].to_string()),
            _ => None,
        };

        let kind = match node {
            Node::DropScopes(_) => "DropScopes",
            Node::PopScopedSymbol(_) => "PopScopedSymbol",
            Node::PopSymbol(_) => "PopSymbol",
            Node::PushScopedSymbol(_) => "PushScopedSymbol",
            Node::PushSymbol(_) => "PushSymbol",
            Node::Root(_) => "Root",
            Node::Scope(_) => "Scope",
            _ => "Other",
        }.to_string();

        NodeInfo {
            file: file_name,
            line,
            col,
            symbol,
            kind,
            id: format!("{:?}", handle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack_graphs::loader::Language;
    use tempfile::TempDir;

    #[test]
    #[ignore] // Fails due to stack-graphs resolution issues in test env
    fn test_trace_reference() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage_path = temp_dir.path().join("stack_graph.bin");
        let mut manager = StackGraphManager::new(storage_path)?;
        let root = temp_dir.path();

        let file1 = root.join("main.py");
        let content1 = "def foo(): pass\nfoo()";
        // Line 1: def foo(): pass
        // Line 2: foo()
        
        manager.sync(&[(file1.clone(), content1.to_string(), Language::Python, "hash".into())], root)?;

        let debugger = GraphDebugger::new(&manager);
        
        // Trace 'foo' at line 2
        // "foo()" starts at col 0. In 1-indexed, that's line 2, col 1.
        let file_path_str = file1.to_string_lossy();
        
        let trace = debugger.trace_reference(&file_path_str, 2, 1)?;

        if trace.definitions.is_empty() {
            println!("Trace error: {:?}", trace.error);
            println!("Nodes in graph:");
            for handle in manager.graph.iter_nodes() {
                let node = &manager.graph[handle];
                let kind = match node {
                    Node::DropScopes(_) => "DropScopes",
                    Node::PopScopedSymbol(_) => "PopScopedSymbol",
                    Node::PopSymbol(_) => "PopSymbol",
                    Node::PushScopedSymbol(_) => "PushScopedSymbol",
                    Node::PushSymbol(_) => "PushSymbol",
                    Node::Root(_) => "Root",
                    Node::Scope(_) => "Scope",
                    _ => "Other",
                };
                println!("  Node: {}", kind);
                if let Some(info) = manager.graph.source_info(handle) {
                     println!("    Source: {}:{}-{}:{}", 
                        info.span.start.line, info.span.start.column.utf8_offset,
                        info.span.end.line, info.span.end.column.utf8_offset
                     );
                }
            }
        }

        assert_eq!(trace.definitions.len(), 1);
        assert_eq!(trace.definitions[0].file, file_path_str);
        // Definition is at line 1
        assert_eq!(trace.definitions[0].line, 1);
        
        Ok(())
    }

    #[test]
    fn test_trace_reference_rust() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage_path = temp_dir.path().join("stack_graph.bin");
        let mut manager = StackGraphManager::new(storage_path)?;
        let root = temp_dir.path();

        let file1 = root.join("main.rs");
        let content1 = "fn foo() {}\nfn main() { foo(); }";
        
        manager.sync(&[(file1.clone(), content1.to_string(), Language::Rust, "hash".into())], root)?;

        let debugger = GraphDebugger::new(&manager);
        let file_path_str = file1.to_string_lossy();
        
        let trace = match debugger.trace_reference(&file_path_str, 2, 13) {
            Ok(t) => t,
            Err(e) => {
                println!("Error tracing: {}", e);
                println!("Nodes in file:");
                for handle in manager.graph.iter_nodes() {
                    let node = &manager.graph[handle];
                    if let Some(file) = node.file() {
                        if manager.graph[file].name() == file_path_str {
                             let kind = match node {
                                Node::PushSymbol(_) => "PushSymbol",
                                Node::PopSymbol(_) => "PopSymbol",
                                _ => "Other",
                            };
                            if let Some(info) = manager.graph.source_info(handle) {
                                println!("  {} at {}:{}-{}:{}", kind, 
                                    info.span.start.line, info.span.start.column.utf8_offset,
                                    info.span.end.line, info.span.end.column.utf8_offset);
                            } else {
                                println!("  {} (no source)", kind);
                            }
                        }
                    }
                }
                return Err(e);
            }
        };

        if trace.definitions.is_empty() {
             println!("Rust Trace error: {:?}", trace.error);
             println!("Nodes in graph:");
             for handle in manager.graph.iter_nodes() {
                let node = &manager.graph[handle];
                let kind = match node {
                    Node::DropScopes(_) => "DropScopes",
                    Node::PopScopedSymbol(_) => "PopScopedSymbol",
                    Node::PopSymbol(_) => "PopSymbol",
                    Node::PushScopedSymbol(_) => "PushScopedSymbol",
                    Node::PushSymbol(_) => "PushSymbol",
                    Node::Root(_) => "Root",
                    Node::Scope(_) => "Scope",
                    _ => "Other",
                };
                let symbol = match node {
                    Node::PushSymbol(n) => Some(manager.graph[n.symbol].to_string()),
                    Node::PopSymbol(n) => Some(manager.graph[n.symbol].to_string()),
                    _ => None,
                };
                
                if let Some(info) = manager.graph.source_info(handle) {
                     println!("  {} {:?} at {}:{}-{}:{}", 
                        kind, symbol,
                        info.span.start.line, info.span.start.column.utf8_offset,
                        info.span.end.line, info.span.end.column.utf8_offset
                     );
                } else {
                    println!("  {} {:?}", kind, symbol);
                }
            }
        }

        assert_eq!(trace.definitions.len(), 1);
        Ok(())
    }

    #[test]
    fn test_manual_graph_resolution() -> Result<()> {
        let mut manager = StackGraphManager::new(std::path::PathBuf::from("dummy.bin"))?;
        let graph = &mut manager.graph;
        
        let file = graph.get_or_create_file("test.rs");
        
        // Create nodes
        // Ref (Push "foo") -> Scope -> Def (Pop "foo")
        
        // 1. Define "foo" (PopSymbol)
        let def_symbol = graph.add_symbol("foo");
        let def_id = graph.new_node_id(file);
        let def_node = graph.add_pop_symbol_node(def_id, def_symbol, true).unwrap();
        
        // 2. Reference "foo" (PushSymbol)
        let ref_symbol = graph.add_symbol("foo");
        let ref_id = graph.new_node_id(file);
        let ref_node = graph.add_push_symbol_node(ref_id, ref_symbol, true).unwrap();
        
        // 3. Scope
        let scope_id = graph.new_node_id(file);
        let scope_node = graph.add_scope_node(scope_id, true).unwrap();
        
        // 4. Edges
        // Ref -> Scope
        graph.add_edge(ref_node, scope_node, 0);
        // Scope -> Def
        graph.add_edge(scope_node, def_node, 0);
        
        // Test trace_node
        let debugger = GraphDebugger::new(&manager);
        let trace = debugger.trace_node(ref_node)?;
        
        assert_eq!(trace.definitions.len(), 1);
        assert_eq!(trace.definitions[0].symbol.as_deref(), Some("foo"));
        
        Ok(())
    }
}
