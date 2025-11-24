use anyhow::Result;
use async_trait::async_trait;
use coderet_config::SummaryLevel;
use coderet_core::models::{Chunk, Summary};
use coderet_core::ranking::RankConfig;
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
        if text == "query" {
            Ok(vec![1.0, 0.0])
        } else {
            Ok(vec![0.5, 0.5])
        }
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                if t == "query" {
                    vec![1.0, 0.0]
                } else {
                    vec![0.5, 0.5]
                }
            })
            .collect())
    }
}

#[tokio::test]
async fn test_contextual_search() -> Result<()> {
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

    // 1. Add Data
    // Add a chunk
    let chunk = Chunk {
        id: "chunk1".to_string(),
        language: coderet_core::models::Language::Rust,
        file_path: std::path::PathBuf::from("test.rs"),
        start_line: 1,
        end_line: 10,
        start_byte: None,
        end_byte: None,
        node_type: "function".to_string(),
        content_hash: "hash1".to_string(),
        content: "fn test() {}".to_string(),
        embedding: Some(vec![1.0, 0.0]),
        parent_scope: None,
        scope_path: vec![],
    };

    let mut txn = manager.begin_transaction().await?;
    txn.add_chunk(chunk.clone(), 1)?;
    txn.commit().await?;

    // Add a summary directly via Mutex lock
    let summary_item = Summary {
        id: "sum1".to_string(),
        target_id: "chunk1".to_string(),
        level: SummaryLevel::Function,
        text: "This is a summary of test function".to_string(),
        file_path: Some(std::path::PathBuf::from("test.rs")),
        start_line: Some(1),
        end_line: Some(10),
        name: Some("test".to_string()),
        language: Some("rust".to_string()),
        module: Some("tests".to_string()),
        model: None,
        prompt_version: None,
        generated_at: None,
        source_hash: None,
        embedding: Some(vec![1.0, 0.0]),
        canonical_target_id: None,
    };

    {
        let mut guard = summary.lock().await;
        guard.add_summaries(&[summary_item]).await?;
    }

    // 2. Search Contextual
    let mut rank_cfg = RankConfig::default();
    rank_cfg.summary_boost_weight = 0.5;
    rank_cfg.summary_similarity_threshold = 0.8;

    let result = manager
        .search_contextual("query", 5, Some(rank_cfg))
        .await?;

    // Verify Chunks
    assert!(!result.chunks.is_empty());
    let top_chunk = &result.chunks[0];
    assert_eq!(top_chunk.chunk.id, "chunk1");
    // Check if boosted (base score should be ~0.4 from vector + lexical, plus 0.5 boost)
    // Vector score for [1.0, 0.0] vs [1.0, 0.0] is 1.0.
    // Lexical score might be 0 if BM25 doesn't match "query" in "fn test() {}".
    // So base score ~0.4. Boosted by 0.5 -> ~0.9.
    assert!(top_chunk.score > 0.5, "Score should be boosted");
    assert!(
        top_chunk.summary_score.is_some(),
        "Summary score should be present"
    );

    // Verify Summaries
    assert!(!result.summaries.is_empty());
    assert_eq!(result.summaries[0].id, "sum1");

    Ok(())
}
