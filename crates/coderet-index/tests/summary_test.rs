use anyhow::Result;
use async_trait::async_trait;
use coderet_config::SummaryLevel;
use coderet_core::models::Summary;
use coderet_core::traits::Embedder;
use coderet_index::summaries::SummaryIndex;

use tempfile::tempdir;

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
async fn test_summary_index_lance() -> Result<()> {
    let dir = tempdir()?;
    let index_path = dir.path().join("summary_index.lance");

    let mut index = SummaryIndex::new(&index_path).await?;

    let summary = Summary {
        id: "1".to_string(),
        target_id: "target1".to_string(),
        level: SummaryLevel::Function,
        text: "This is a summary".to_string(),
        file_path: Some(std::path::PathBuf::from("test.rs")),
        start_line: Some(1),
        end_line: Some(10),
        name: Some("test_func".to_string()),
        language: Some("rust".to_string()),
        module: Some("tests".to_string()),
        model: None,
        prompt_version: None,
        generated_at: None,
        source_hash: None,
        embedding: Some(vec![1.0, 0.0]), // Matches "query"
        canonical_target_id: None,
    };

    index.add_summaries(&[summary.clone()]).await?;

    let embedder = MockEmbedder;
    let results = index.semantic_search("query", &embedder, 1).await?;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.id, "1");
    assert!(results[0].0 > 0.9); // High similarity

    Ok(())
}

#[tokio::test]
async fn structured_search_returns_metadata() -> Result<()> {
    let dir = tempdir()?;
    let index_path = dir.path().join("summary_index.lance");

    let mut index = SummaryIndex::new(&index_path).await?;

    let summary = Summary {
        id: "file1".to_string(),
        target_id: "file:foo".to_string(),
        level: SummaryLevel::File,
        text: "foo summary".to_string(),
        file_path: Some(std::path::PathBuf::from("src/foo.rs")),
        start_line: Some(1),
        end_line: Some(5),
        name: Some("foo".to_string()),
        language: Some("rust".to_string()),
        module: Some("src".to_string()),
        model: None,
        prompt_version: None,
        generated_at: None,
        source_hash: None,
        embedding: Some(vec![1.0, 0.0]),
        canonical_target_id: None,
    };
    index.add_summaries(&[summary.clone()]).await?;

    let embedder = MockEmbedder;
    let results = index
        .search_structured("foo", 5, Some(&embedder))
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    let record = &results[0].1;
    assert_eq!(record.kind, coderet_index::summaries::SummaryKind::File);
    assert_eq!(record.target_id, "file:foo");
    assert_eq!(record.module.as_deref(), Some("src"));
    assert_eq!(record.language, Some(coderet_core::models::Language::Rust));
    assert_eq!(
        record.file_path.as_ref().unwrap(),
        &std::path::PathBuf::from("src/foo.rs")
    );

    // Repo/module helper surfaces repo-level entries.
    let repo_summary = Summary {
        id: "repo".to_string(),
        target_id: "repo:root".to_string(),
        level: SummaryLevel::Repo,
        text: "repo summary".to_string(),
        file_path: None,
        start_line: None,
        end_line: None,
        name: Some("repo".to_string()),
        language: None,
        module: None,
        model: None,
        prompt_version: None,
        generated_at: None,
        source_hash: None,
        embedding: Some(vec![0.5, 0.5]),
        canonical_target_id: None,
    };
    index.add_summaries(&[repo_summary]).await?;
    let top = index.get_repo_and_module_summaries(4).await?;
    assert!(top
        .iter()
        .any(|r| matches!(r.kind, coderet_index::summaries::SummaryKind::Repo)));

    Ok(())
}
