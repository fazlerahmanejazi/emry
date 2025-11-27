use coderet_graph::graph::CodeGraph;
use std::path::PathBuf;

fn build_graph() -> CodeGraph {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.bin");
    
    let mut graph = CodeGraph::new(path);
    graph
        .add_node(coderet_graph::graph::GraphNode {
            id: "A".to_string(),
            kind: "file".to_string(),
            label: "A".to_string(),
            canonical_id: None,
            file_path: "a.rs".to_string(),
        })
        .unwrap();
    graph
        .add_node(coderet_graph::graph::GraphNode {
            id: "B".to_string(),
            kind: "symbol".to_string(),
            label: "B".to_string(),
            canonical_id: None,
            file_path: "b.rs".to_string(),
        })
        .unwrap();
    graph
        .add_node(coderet_graph::graph::GraphNode {
            id: "C".to_string(),
            kind: "symbol".to_string(),
            label: "C".to_string(),
            canonical_id: None,
            file_path: "c.rs".to_string(),
        })
        .unwrap();
    graph
        .add_node(coderet_graph::graph::GraphNode {
            id: "D".to_string(),
            kind: "symbol".to_string(),
            label: "D".to_string(),
            canonical_id: None,
            file_path: "d.rs".to_string(),
        })
        .unwrap();

    graph.add_edge("A", "B", "calls").unwrap();
    graph.add_edge("B", "C", "calls").unwrap();
    graph.add_edge("B", "D", "imports").unwrap();
    graph
}

#[test]
fn neighbors_basic() {
    let graph = build_graph();
    let neighbors = graph.get_neighbors("A").unwrap();
    let node_ids: std::collections::HashSet<_> = neighbors.iter().map(|n| n.id.as_str()).collect();
    assert!(node_ids.contains("B"));
    
    let neighbors_b = graph.get_neighbors("B").unwrap();
    let node_ids_b: std::collections::HashSet<_> = neighbors_b.iter().map(|n| n.id.as_str()).collect();
    assert!(node_ids_b.contains("C"));
    assert!(node_ids_b.contains("D"));
}

#[test]
fn shortest_path_basic() {
    let graph = build_graph();
    let path = graph.shortest_path("A", "C", 5).unwrap().unwrap();
    let ids: Vec<String> = path.iter().map(|n| n.id.clone()).collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}

#[test]
fn persistence_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.bin");
    
    {
        let mut graph = CodeGraph::new(path.clone());
        graph.add_node(coderet_graph::graph::GraphNode {
            id: "X".to_string(),
            kind: "test".to_string(),
            label: "X".to_string(),
            canonical_id: None,
            file_path: "x.rs".to_string(),
        }).unwrap();
        graph.save().unwrap();
    }
    
    {
        let graph = CodeGraph::load(&path).unwrap();
        let node = graph.get_node("X").unwrap();
        assert!(node.is_some());
        assert_eq!(node.unwrap().label, "X");
    }
}