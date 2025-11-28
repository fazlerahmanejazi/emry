use coderet_context::RepoContext;
use coderet_tools::fs::FsTool;
use coderet_config::Config;
use coderet_store::storage::Store;
use coderet_store::file_store::FileStore;
use coderet_store::content_store::ContentStore;
use coderet_store::file_blob_store::FileBlobStore;
use coderet_store::chunk_store::ChunkStore;
// use coderet_store::relation_store::RelationStore; // Removed
use coderet_graph::graph::CodeGraph;
use coderet_index::lexical::LexicalIndex;
use coderet_index::vector::VectorIndex;
use coderet_index::summaries::SummaryIndex;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
use tempfile::TempDir;

async fn setup_fs_tool() -> (FsTool, TempDir, TempDir) {
    let root_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let root_path = root_dir.path().canonicalize().unwrap(); // Canonicalize to resolve symlinks
    let index_path = index_dir.path().to_path_buf();
    
    // Create a nested directory structure
    // root/
    //   file1.txt
    //   dir1/
    //     file2.txt
    //     dir2/
    //       file3.txt
    
    fs::write(root_path.join("file1.txt"), "content1").unwrap();
    fs::create_dir(root_path.join("dir1")).unwrap();
    fs::write(root_path.join("dir1/file2.txt"), "content2").unwrap();
    fs::create_dir(root_path.join("dir1/dir2")).unwrap();
    fs::write(root_path.join("dir1/dir2/file3.txt"), "content3").unwrap();

    // Setup minimal RepoContext
    let db_path = index_path.join("store.db");
    let store = Store::open(&db_path).unwrap();
    
    let file_store = Arc::new(FileStore::new(store.clone()).unwrap());
    let content_store = Arc::new(ContentStore::new(store.clone()).unwrap());
    let file_blob_store = Arc::new(FileBlobStore::new(store.clone()).unwrap());
    let chunk_store = Arc::new(ChunkStore::new(store.clone()).unwrap());
    // let relation_store = Arc::new(RelationStore::new(store.clone()).unwrap()); // Removed
    
    let graph_path = index_path.join("graph.bin");
    let graph = Arc::new(RwLock::new(CodeGraph::new(graph_path)));
    
    let lexical = Arc::new(LexicalIndex::new(&index_path.join("lexical")).unwrap());
    let vector = Arc::new(Mutex::new(VectorIndex::new(&index_path.join("vector.lance")).await.unwrap()));
    let summary_index = Arc::new(Mutex::new(SummaryIndex::new(&index_path.join("summaries.db")).await.unwrap()));

    let ctx = Arc::new(RepoContext {
        root: root_path,
        branch: "main".to_string(),
        index_dir: index_path,
        config: Config::default(),
        graph,
        file_store,
        content_store,
        file_blob_store,
        chunk_store,
        // relation_store,
        summary_index,
        commit_log: None,
        embedder: None,
        lexical,
        vector,
    });

    let fs_tool = FsTool::new(ctx);

    (fs_tool, root_dir, index_dir)
}

#[tokio::test]
async fn test_list_files_depth_1() {
    let (fs_tool, _root, _index) = setup_fs_tool().await;
    
    // Depth 1 should only show file1.txt and dir1
    let entries = fs_tool.list_files(Path::new("."), 1, None).unwrap();
    
    assert_eq!(entries.len(), 2);
    let paths: Vec<_> = entries.iter().map(|e| e.path.file_name().unwrap().to_str().unwrap()).collect();
    assert!(paths.contains(&"file1.txt"));
    assert!(paths.contains(&"dir1"));
    assert!(!paths.contains(&"file2.txt"));
}

#[tokio::test]
async fn test_list_files_depth_2() {
    let (fs_tool, _root, _index) = setup_fs_tool().await;
    
    // Depth 2 should show file1.txt, dir1, and dir1/file2.txt, dir1/dir2
    let entries = fs_tool.list_files(Path::new("."), 2, None).unwrap();
    
    // file1.txt, dir1, file2.txt, dir2 = 4 entries
    assert_eq!(entries.len(), 4);
    
    let paths: Vec<_> = entries.iter().map(|e| e.path.file_name().unwrap().to_str().unwrap()).collect();
    assert!(paths.contains(&"file1.txt"));
    assert!(paths.contains(&"dir1"));
    assert!(paths.contains(&"file2.txt"));
    assert!(paths.contains(&"dir2"));
    assert!(!paths.contains(&"file3.txt"));
}

#[tokio::test]
async fn test_list_files_depth_3() {
    let (fs_tool, _root, _index) = setup_fs_tool().await;
    
    // Depth 3 should show everything including file3.txt
    let entries = fs_tool.list_files(Path::new("."), 3, None).unwrap();
    
    // file1.txt, dir1, file2.txt, dir2, file3.txt = 5 entries
    assert_eq!(entries.len(), 5);
    
    let paths: Vec<_> = entries.iter().map(|e| e.path.file_name().unwrap().to_str().unwrap()).collect();
    assert!(paths.contains(&"file3.txt"));
}

#[tokio::test]
async fn test_list_files_outside_workspace() {
    let (fs_tool, _root, _index) = setup_fs_tool().await;
    
    // Test list_files with path outside workspace
    let result = fs_tool.list_files(Path::new("/tmp"), 1, None);
    assert!(result.is_err(), "list_files outside workspace should fail");
}

#[tokio::test]
async fn test_list_files_nonexistent() {
    let (fs_tool, _root, _index) = setup_fs_tool().await;
    
    // Test list_files with nonexistent path
    let result = fs_tool.list_files(Path::new("nonexistent"), 1, None);
    assert!(result.is_err(), "list_files nonexistent should fail");
}
