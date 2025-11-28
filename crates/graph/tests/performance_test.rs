use emry_graph::graph::{CodeGraph, GraphNode};
use std::path::PathBuf;
use std::time::Instant;

#[test]
fn test_performance_indices() {
    let mut graph = CodeGraph::new(PathBuf::from("test_graph.bin"));
    let num_files = 100;
    let nodes_per_file = 100;
    
    // 1. Populate graph
    let start = Instant::now();
    for i in 0..num_files {
        let file_path = format!("/path/to/file_{}.rs", i);
        // Add file node
        graph.add_node(GraphNode {
            id: file_path.clone(),
            kind: "file".to_string(),
            label: format!("file_{}.rs", i),
            canonical_id: None,
            file_path: file_path.clone(),
        }).unwrap();
        
        // Add symbol nodes
        for j in 0..nodes_per_file {
            graph.add_node(GraphNode {
                id: format!("{}#symbol_{}", file_path, j),
                kind: "symbol".to_string(),
                label: format!("symbol_{}", j),
                canonical_id: None,
                file_path: file_path.clone(),
            }).unwrap();
        }
    }
    println!("Populated {} nodes in {:?}", num_files * (nodes_per_file + 1), start.elapsed());
    
    // 2. Test list_symbols performance
    let start = Instant::now();
    let symbols = graph.list_symbols().unwrap();
    let duration = start.elapsed();
    println!("Listed {} symbols in {:?}", symbols.len(), duration);
    assert_eq!(symbols.len(), num_files * nodes_per_file);
    // Expect it to be very fast (sub-millisecond for 10k nodes)
    assert!(duration.as_millis() < 50, "list_symbols took too long: {:?}", duration);
    
    // 3. Test delete_nodes_for_file performance
    let target_file = "/path/to/file_50.rs";
    let start = Instant::now();
    graph.delete_nodes_for_file(target_file).unwrap();
    let duration = start.elapsed();
    println!("Deleted nodes for file in {:?}", duration);
    
    // Verify deletion
    let remaining_symbols = graph.list_symbols().unwrap();
    assert_eq!(remaining_symbols.len(), (num_files - 1) * nodes_per_file);
    assert!(duration.as_millis() < 50, "delete_nodes_for_file took too long: {:?}", duration);
}
