use core::structure::graph::{CodeGraph, NodeId, NodeType, EdgeType};
use core::paths::builder::{PathBuilder, PathBuilderConfig};
use std::path::PathBuf;

#[test]
fn test_path_builder_simple_flow() {
    let mut graph = CodeGraph::default();

    // Create nodes
    let id_a = NodeId("A".to_string());
    let id_b = NodeId("B".to_string());
    let id_c = NodeId("C".to_string());

    graph.add_node(id_a.clone(), NodeType::Function, "FuncA".to_string(), Some(PathBuf::from("a.rs")), 1, 10);
    graph.add_node(id_b.clone(), NodeType::Function, "FuncB".to_string(), Some(PathBuf::from("b.rs")), 1, 10);
    graph.add_node(id_c.clone(), NodeType::Function, "FuncC".to_string(), Some(PathBuf::from("c.rs")), 1, 10);

    // Create edges: A -> B -> C
    graph.add_edge(id_a.clone(), id_b.clone(), EdgeType::Calls);
    graph.add_edge(id_b.clone(), id_c.clone(), EdgeType::Calls);

    // Build paths
    let builder = PathBuilder::new(&graph);
    let config = PathBuilderConfig::default();
    
    let paths = builder.find_paths(&id_a, &config);

    // Expect:
    // 1. A -> B
    // 2. A -> B -> C
    
    assert!(paths.len() >= 2, "Should find at least 2 paths");
    
    let path_abc = paths.iter().find(|p| p.nodes.len() == 3).expect("Should find path A->B->C");
    assert_eq!(path_abc.nodes[0].node_id, "A");
    assert_eq!(path_abc.nodes[1].node_id, "B");
    assert_eq!(path_abc.nodes[2].node_id, "C");
    
    assert_eq!(path_abc.edges[0].kind, EdgeType::Calls);
    assert_eq!(path_abc.edges[1].kind, EdgeType::Calls);
}

#[test]
fn test_path_builder_cycle() {
    let mut graph = CodeGraph::default();
    let id_a = NodeId("A".to_string());
    let id_b = NodeId("B".to_string());

    graph.add_node(id_a.clone(), NodeType::Function, "A".to_string(), None, 0, 0);
    graph.add_node(id_b.clone(), NodeType::Function, "B".to_string(), None, 0, 0);

    // A <-> B
    graph.add_edge(id_a.clone(), id_b.clone(), EdgeType::Calls);
    graph.add_edge(id_b.clone(), id_a.clone(), EdgeType::Calls);

    let builder = PathBuilder::new(&graph);
    let config = PathBuilderConfig::default();
    let paths = builder.find_paths(&id_a, &config);

    // Should find A->B, but not infinite loop
    assert!(!paths.is_empty());
    for p in paths {
        assert!(p.nodes.len() <= config.max_length + 1);
    }
}
