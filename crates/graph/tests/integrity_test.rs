use anyhow::Result;
use emry_graph::graph::{CodeGraph, GraphNode};
use std::collections::HashSet;
use tempfile::TempDir;

#[test]
fn test_graph_integrity_stress() -> Result<()> {
    // 1. Setup
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("graph.bin");
    let mut graph = CodeGraph::new(db_path.clone());

    // 2. Generate Data (Small stress test)
    // 100 files, each with 10 symbols
    let num_files = 100;
    let symbols_per_file = 10;
    
    let mut expected_files = HashSet::new();
    let mut expected_symbols = HashSet::new();

    for f in 0..num_files {
        let file_path = format!("/src/file_{}.rs", f);
        let file_id = format!("file:{}", f);
        
        graph.add_node(GraphNode {
            id: file_id.clone(),
            kind: "file".to_string(),
            label: file_path.clone(),
            canonical_id: Some(file_id.clone()),
            file_path: file_path.clone(),
        })?;
        expected_files.insert(file_path.clone());

        for s in 0..symbols_per_file {
            let symbol_id = format!("symbol:{}:{}", f, s);
            let label = format!("Symbol_{}_{}", f, s);
            
            graph.add_node(GraphNode {
                id: symbol_id.clone(),
                kind: "symbol".to_string(),
                label: label.clone(),
                canonical_id: Some(symbol_id.clone()),
                file_path: file_path.clone(),
            })?;
            expected_symbols.insert(symbol_id.clone());
            
            // Link file -> symbol
            graph.add_edge(&file_id, &symbol_id, "defines")?;
        }
    }

    // 3. Verify Initial State
    // We can't access private fields `files_to_nodes` directly, 
    // but we can verify behavior via public methods.
    
    // Check list_symbols count
    let symbols = graph.list_symbols()?;
    assert_eq!(symbols.len(), num_files * symbols_per_file);
    
    // 4. Random Deletions
    // Delete every even file
    for f in (0..num_files).step_by(2) {
        let file_path = format!("/src/file_{}.rs", f);
        graph.delete_nodes_for_file(&file_path)?;
        expected_files.remove(&file_path);
        
        // Removing file should remove its symbols too (if delete_nodes_for_file is implemented to do so)
        // Let's check implementation of delete_nodes_for_file.
        // It removes nodes mapped to that file.
        for s in 0..symbols_per_file {
            let symbol_id = format!("symbol:{}:{}", f, s);
            expected_symbols.remove(&symbol_id);
        }
    }

    // 5. Verify Integrity
    let remaining_symbols = graph.list_symbols()?;
    assert_eq!(remaining_symbols.len(), expected_symbols.len(), "Symbol count mismatch after deletion");
    
    for sym in remaining_symbols {
        assert!(expected_symbols.contains(&sym.id), "Found unexpected symbol: {}", sym.id);
    }

    // Verify file nodes are gone
    // We don't have a direct "list_files" but we can check if we can resolve them.
    for f in 0..num_files {
        let file_id = format!("file:{}", f);
        let node = graph.get_node(&file_id)?;
        if f % 2 == 0 {
            assert!(node.is_none(), "Deleted file node {} should be gone", file_id);
        } else {
            assert!(node.is_some(), "Kept file node {} should exist", file_id);
        }
    }

    // 6. Persistence Check
    graph.save()?;
    let loaded_graph = CodeGraph::load(&db_path)?;
    
    let loaded_symbols = loaded_graph.list_symbols()?;
    assert_eq!(loaded_symbols.len(), expected_symbols.len(), "Persistence failed to preserve state");

    Ok(())
}
