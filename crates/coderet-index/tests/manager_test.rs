use anyhow::Result;
use async_trait::async_trait;
use coderet_config::SummaryLevel;
use coderet_core::models::{Chunk, Language};

use coderet_core::traits::Embedder;
use coderet_graph::graph::CodeGraph;
use coderet_index::lexical::LexicalIndex;
use coderet_index::manager::IndexManager;
use coderet_index::summaries::SummaryIndex;
use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::content_store::ContentStore;
use coderet_store::file_blob_store::FileBlobStore;
use coderet_store::file_store::FileStore;
use coderet_store::relation_store::RelationStore;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;

struct MockEmbedder;

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if text.contains("query") {
            Ok(vec![1.0, 0.0])
        } else {
            Ok(vec![0.5, 0.5])
        }
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                if t.contains("query") {
                    vec![1.0, 0.0]
                } else {
                    vec![0.5, 0.5]
                }
            })
            .collect())
    }
}

#[tokio::test]
async fn test_index_manager_crud_and_search() -> Result<()> {
    let dir = tempdir()?;

    // Setup Stores
    let db = sled::open(dir.path().join("store"))?;
    let file_store = Arc::new(FileStore::new(db.clone())?);
    let chunk_store = Arc::new(ChunkStore::new(db.clone())?);
    let content_store = Arc::new(ContentStore::new(db.clone())?);
    let file_blob_store = Arc::new(FileBlobStore::new(db.clone())?);
    let relation_store = Arc::new(RelationStore::new(db.clone())?);

    // Setup Indices
    let lexical = Arc::new(LexicalIndex::new(&dir.path().join("lexical"))?);
    let vector = Arc::new(Mutex::new(
        VectorIndex::new(&dir.path().join("vector.lance")).await?,
    ));
    let summary = Arc::new(Mutex::new(
        SummaryIndex::new(&dir.path().join("summary.lance")).await?,
    ));
    let graph = Arc::new(CodeGraph::new(db.clone())?);
    let embedder = Arc::new(MockEmbedder);

    let manager = IndexManager::new(
        lexical,
        vector,
        Some(embedder.clone()),
        file_store,
        chunk_store,
        content_store,
        file_blob_store,
        relation_store,
        graph.clone(),
        Some(summary.clone()),
    );

    // 1. Add Chunks
    let chunk1 = Chunk {
        id: "chunk1".to_string(),
        language: Language::Rust,
        file_path: std::path::PathBuf::from("test.rs"),
        start_line: 1,
        end_line: 5,
        start_byte: None,
        end_byte: None,
        node_type: "function".to_string(),
        content_hash: "hash1".to_string(),
        content: "fn test_query() {}".to_string(), // Contains "query"
        embedding: Some(vec![1.0, 0.0]),
        parent_scope: None,
        scope_path: vec![],
    };

    let chunk2 = Chunk {
        id: "chunk2".to_string(),
        language: Language::Rust,
        file_path: std::path::PathBuf::from("other.rs"),
        start_line: 1,
        end_line: 5,
        start_byte: None,
        end_byte: None,
        node_type: "function".to_string(),
        content_hash: "hash2".to_string(),
        content: "fn other() {}".to_string(),
        embedding: Some(vec![0.5, 0.5]),
        parent_scope: None,
        scope_path: vec![],
    };

    let mut txn = manager.begin_transaction().await?;
    txn.add_chunk(chunk1.clone(), 1)?;
    txn.add_chunk(chunk2.clone(), 2)?;
    txn.commit().await?;

    // 2. Search
    // Hybrid search for "query"
    let results = manager.search("query", 5).await?;
    assert_eq!(results.len(), 2); // Both might match vector search somewhat, but chunk1 should be top
    assert_eq!(results[0].1.id, "chunk1");
    assert!(results[0].0 > results[1].0);

    // 3. Delete Chunk
    let mut txn = manager.begin_transaction().await?;
    txn.delete_chunks(vec!["chunk1".to_string()]);
    txn.commit().await?;

    // 4. Search Again
    let results = manager.search("query", 5).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.id, "chunk2");

    Ok(())
}
