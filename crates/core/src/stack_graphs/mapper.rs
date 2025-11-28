use anyhow::Result;
use stack_graphs::graph::{StackGraph, Node};
use stack_graphs::arena::Handle;
use stack_graphs::partial::PartialPaths;
use stack_graphs::stitching::{ForwardPartialPathStitcher, GraphEdgeCandidates, StitcherConfig};
use stack_graphs::NoCancellation;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    pub file_path: String,
    pub kind: String, // "function", "struct", "method", etc.
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub from_file: String,
    pub to_symbol: String,
    pub to_file: String,
}

pub struct GraphMapper<'a> {
    stack_graph: &'a StackGraph,
}

impl<'a> GraphMapper<'a> {
    pub fn new(stack_graph: &'a StackGraph) -> Self {
        Self { stack_graph }
    }

    pub fn extract_symbols(&self) -> Result<Vec<SymbolInfo>> {
        let mut symbols = Vec::new();
        
        for node_handle in self.stack_graph.iter_nodes() {
            let node = &self.stack_graph[node_handle];
            match node {
                Node::PopSymbol(pop) => {
                    if pop.is_definition {
                        symbols.push(self.create_symbol_info(node_handle, &pop.symbol)?);
                    }
                }
                Node::PopScopedSymbol(pop) => {
                    if pop.is_definition {
                        symbols.push(self.create_symbol_info(node_handle, &pop.symbol)?);
                    }
                }
                _ => {}
            }
        }

        Ok(symbols)
    }

    pub fn extract_calls(&self) -> Result<Vec<CallEdge>> {
        let mut call_edges = Vec::new();
        let mut partials = PartialPaths::new();
        
        // Find all reference nodes (these are the "calls")
        let references: Vec<Handle<Node>> = self.stack_graph
            .iter_nodes()
            .filter(|handle| self.stack_graph[*handle].is_reference())
            .collect();
        
        if references.is_empty() {
            return Ok(call_edges);
        }
        
        // Use path finding to resolve references to definitions
        ForwardPartialPathStitcher::find_all_complete_partial_paths(
            &mut GraphEdgeCandidates::new(self.stack_graph, &mut partials, None),
            references,
            StitcherConfig::default(),
            &NoCancellation,
            |graph, _partials, path| {
                // Extract call edge from complete path (reference -> definition)
                let start_node = &graph[path.start_node];
                let end_node = &graph[path.end_node];
                
                // Get file information
                if let (Some(start_file), Some(end_file)) = (start_node.file(), end_node.file()) {
                    let from_file = graph[start_file].name().to_string();
                    let to_file = graph[end_file].name().to_string();
                    
                    // Get symbol name from the definition
                    let to_symbol = match end_node {
                        Node::PopSymbol(pop) => graph[pop.symbol].to_string(),
                        Node::PopScopedSymbol(pop) => graph[pop.symbol].to_string(),
                        _ => return, // Not a definition, skip
                    };
                    
                    call_edges.push(CallEdge {
                        from_file,
                        to_symbol,
                        to_file,
                    });
                }
            },
        ).map_err(|e| anyhow::anyhow!("Path finding failed: {:?}", e))?;
        
        Ok(call_edges)
    }

    fn create_symbol_info(&self, node_handle: Handle<Node>, symbol_handle: &Handle<stack_graphs::graph::Symbol>) -> Result<SymbolInfo> {
        let symbol = &self.stack_graph[*symbol_handle];
        let file_handle = self.stack_graph[node_handle].file().unwrap();
        let file_str = self.stack_graph[file_handle].name();
        
        // Extract source location
        let source_info = self.stack_graph.source_info(node_handle);
        let (start_line, end_line) = if let Some(info) = source_info {
            // Convert from 0-indexed to 1-indexed lines
            (info.span.start.line + 1, info.span.end.line + 1)
        } else {
            // Fallback if no span info
            (1, 1)
        };
        
        Ok(SymbolInfo {
            symbol: symbol.to_string(),
            file_path: file_str.to_string(),
            kind: "symbol".to_string(), // TODO: infer kind from context
            start_line,
            end_line,
        })
    }
}
