use crate::structure::graph::{CodeGraph, NodeId, NodeType};
use crate::models::Chunk;
use std::collections::HashSet;

pub struct SeedSelector;

impl SeedSelector {
    /// Selects seed nodes for path traversal based on retrieval results.
    /// 
    /// Strategy:
    /// 1. Map top-ranked chunks to their corresponding Graph Nodes (Functions/Classes).
    /// 2. If a chunk is inside a function, pick that function.
    /// 3. If a chunk is at file level, pick the file node (or main function if detectable).
    /// 4. Deduplicate.
    pub fn select_seeds(graph: &CodeGraph, chunks: &[Chunk], limit: usize) -> Vec<NodeId> {
        let mut seeds = HashSet::new();
        let mut result = Vec::new();

        for chunk in chunks {
            if seeds.len() >= limit {
                break;
            }

            // Find nodes that overlap with this chunk
            // This is a naive O(N) search over nodes. In prod, we'd use an interval tree or the symbol index.
            // For now, we iterate graph nodes matching the file.
            
            let mut best_node: Option<&NodeId> = None;
            let mut best_overlap = 0;

            for node in graph.nodes.values() {
                if let Some(path) = &node.file_path {
                    if path.to_string_lossy() == chunk.file_path.to_string_lossy() {
                        // Check overlap
                        let start = std::cmp::max(node.start_line, chunk.start_line);
                        let end = std::cmp::min(node.end_line, chunk.end_line);
                        
                        if start <= end {
                            let overlap = end - start + 1;
                            // Prefer functions over classes/files for flow starting points
                            let type_bonus = match node.kind {
                                NodeType::Function => 1000,
                                NodeType::Class => 500,
                                _ => 0,
                            };
                            let score = overlap + type_bonus;
                            
                            if score > best_overlap {
                                best_overlap = score;
                                best_node = Some(&node.id);
                            }
                        }
                    }
                }
            }

            if let Some(id) = best_node {
                if seeds.insert(id.clone()) {
                    result.push(id.clone());
                }
            }
        }
        
        result
    }
}
