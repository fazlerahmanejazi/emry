use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use coderet_agent::context::RepoContext;
use coderet_agent::tools::{graph::GraphTool, search::SearchTool, summaries::SummaryTool};
use coderet_config::Config;
use coderet_config::SummaryLevel;
use coderet_core::models::Summary;
use coderet_graph::graph::CodeGraph;
use coderet_index::lexical::LexicalIndex;
use coderet_index::manager::IndexManager;
use coderet_index::summaries::SummaryIndex;
use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::commit_log::CommitLog;
use coderet_store::content_store::ContentStore;
use coderet_store::file_blob_store::FileBlobStore;
use coderet_store::file_store::FileStore;
use coderet_store::relation_store::RelationStore;
use tempfile::tempdir;
use tokio::sync::Mutex;

async fn build_repo_ctx() -> RepoContext {
    let root_dir = tempdir().unwrap().into_path();
    let branch = "test".to_string();
    let index_dir = root_dir.join(".codeindex").join("branches").join(&branch);
    std::fs::create_dir_all(&index_dir).unwrap();

    let db_path = index_dir.join("store.db");
    let db = sled::Config::default()
        .path(&db_path)
        .mode(sled::Mode::LowSpace)
        .open()
        .unwrap();

    let file_store = Arc::new(FileStore::new(db.clone()).unwrap());
    let content_store = Arc::new(ContentStore::new(db.clone()).unwrap());
    let file_blob_store = Arc::new(FileBlobStore::new(db.clone()).unwrap());
    let chunk_store = Arc::new(ChunkStore::new(db.clone()).unwrap());
    let relation_store = Arc::new(RelationStore::new(db.clone()).unwrap());
    let graph = Arc::new(CodeGraph::new(db.clone()).unwrap());
    let commit_log = CommitLog::new(db.clone()).ok();

    let lexical = Arc::new(LexicalIndex::new(&index_dir.join("lexical")).unwrap());
    let vector = Arc::new(Mutex::new(
        VectorIndex::new(&index_dir.join("vector.lance"))
            .await
            .unwrap(),
    ));

    let summary_index = Arc::new(Mutex::new(
        SummaryIndex::new(&index_dir.join("summaries.db"))
            .await
            .unwrap(),
    ));

    let manager = Arc::new(IndexManager::new(
        lexical,
        vector,
        None,
        file_store.clone(),
        chunk_store.clone(),
        content_store.clone(),
        file_blob_store.clone(),
        relation_store.clone(),
        graph.clone(),
        Some(summary_index.clone()),
    ));

    RepoContext {
        root: root_dir,
        branch,
        index_dir,
        config: Config::default(),
        manager,
        graph,
        file_store,
        content_store,
        file_blob_store,
        chunk_store,
        relation_store,
        summary_index,
        commit_log,
        embedder: None,
    }
}

#[tokio::test]
async fn summary_tool_searches_structured() {
    let mut ctx = build_repo_ctx().await;

    // Seed a simple file-level summary.
    {
        let mut guard = ctx.summary_index.lock().await;
        guard
            .add_summaries(&[Summary {
                id: "file1".to_string(),
                target_id: "file:src/foo.rs".to_string(),
                level: SummaryLevel::File,
                text: "foo chunked content".to_string(),
                file_path: Some(PathBuf::from("src/foo.rs")),
                start_line: Some(1),
                end_line: Some(5),
                name: Some("foo".to_string()),
                language: Some("rust".to_string()),
                module: Some("src".to_string()),
                model: None,
                prompt_version: None,
                generated_at: None,
                source_hash: None,
                embedding: Some(vec![1.0]),
                canonical_target_id: None,
            }])
            .await
            .unwrap();
    }

    let tool = SummaryTool::new(Arc::new(ctx));
    let results = tool.search_summaries("foo", 5).await.unwrap();
    assert_eq!(results.len(), 1);
    let rec = &results[0].summary;
    assert_eq!(rec.target_id, "file:src/foo.rs");
    assert_eq!(rec.module.as_deref(), Some("src"));
}

#[tokio::test]
async fn search_tool_finds_entry_points() {
    let mut ctx = build_repo_ctx().await;
    ctx.graph
        .add_node(coderet_graph::graph::GraphNode {
            id: "sym1".to_string(),
            kind: "symbol".to_string(),
            label: "main".to_string(),
            canonical_id: None,
            file_path: "src/main.rs".to_string(),
        })
        .unwrap();

    let tool = SearchTool::new(Arc::new(ctx));
    let entries = tool.list_entry_points().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "main");
}

#[tokio::test]
async fn graph_tool_neighbors_filters_kinds() {
    let mut ctx = build_repo_ctx().await;
    let g = &ctx.graph;
    g.add_node(coderet_graph::graph::GraphNode {
        id: "A".into(),
        kind: "file".into(),
        label: "A".into(),
        canonical_id: None,
        file_path: "a.rs".into(),
    })
    .unwrap();
    g.add_node(coderet_graph::graph::GraphNode {
        id: "B".into(),
        kind: "symbol".into(),
        label: "B".into(),
        canonical_id: None,
        file_path: "b.rs".into(),
    })
    .unwrap();
    g.add_edge("A", "B", "calls").unwrap();
    let tool = GraphTool::new(Arc::new(ctx));
    let sub = tool.neighbors("A", &["calls".to_string()], 2).unwrap();
    let ids: HashMap<_, _> = sub
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.label.clone()))
        .collect();
    assert!(ids.contains_key("A"));
    assert!(ids.contains_key("B"));
    assert_eq!(sub.edges.len(), 1);
    assert_eq!(sub.edges[0].kind, "calls");
}

#[tokio::test]
async fn graph_tool_shortest_paths_respects_kinds() {
    let mut ctx = build_repo_ctx().await;
    let g = &ctx.graph;
    for (id, kind) in [("A", "file"), ("B", "symbol"), ("C", "symbol")] {
        g.add_node(coderet_graph::graph::GraphNode {
            id: id.into(),
            kind: kind.into(),
            label: id.into(),
            canonical_id: None,
            file_path: format!("{}.rs", id),
        })
        .unwrap();
    }
    g.add_edge("A", "B", "calls").unwrap();
    g.add_edge("B", "C", "imports").unwrap();
    let tool = GraphTool::new(Arc::new(ctx));
    let calls_only = tool
        .shortest_paths_with_kinds("A", "C", &["calls".to_string()], 4)
        .unwrap();
    // imports edge should block calls-only path
    assert!(calls_only.is_empty());
    let calls_imports = tool
        .shortest_paths_with_kinds("A", "C", &["calls".to_string(), "imports".to_string()], 4)
        .unwrap();
    assert_eq!(calls_imports.len(), 1);
    assert_eq!(
        calls_imports[0],
        vec!["A".to_string(), "B".to_string(), "C".to_string()]
    );
}
