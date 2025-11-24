use coderet_graph::graph::CodeGraph;
use coderet_store::relation_store::RelationType;

fn build_graph() -> CodeGraph {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let graph = CodeGraph::new(db).unwrap();
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
fn neighbors_subgraph_respects_relation_filters_and_hops() {
    let graph = build_graph();
    let sub = graph
        .neighbors_subgraph("A", &[RelationType::Calls], 2)
        .unwrap();
    let node_ids: std::collections::HashSet<_> = sub.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(node_ids.contains("A"));
    assert!(node_ids.contains("B"));
    assert!(node_ids.contains("C"));
    assert!(
        !node_ids.contains("D"),
        "imports edge should be excluded when filtering by calls"
    );
    let edge_kinds: std::collections::HashSet<_> =
        sub.edges.iter().map(|e| e.relation.as_str()).collect();
    assert_eq!(edge_kinds, std::collections::HashSet::from(["calls"]));
}

#[test]
fn shortest_paths_filtered_respects_relation_filters() {
    let graph = build_graph();
    let calls_paths = graph
        .shortest_paths_filtered("A", "C", &[RelationType::Calls], 3)
        .unwrap();
    assert_eq!(calls_paths.len(), 1);
    assert_eq!(
        calls_paths[0],
        vec!["A".to_string(), "B".to_string(), "C".to_string()]
    );

    let import_paths = graph
        .shortest_paths_filtered("A", "C", &[RelationType::Imports], 3)
        .unwrap();
    assert!(
        import_paths.is_empty(),
        "imports-only filter should not find a calls path"
    );
}
