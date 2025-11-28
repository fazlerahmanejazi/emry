use crate::embedder::select_embedder;
use anyhow::{anyhow, Context, Result};
use coderet_config::Config;
use coderet_graph::graph::CodeGraph;
use coderet_index::lexical::LexicalIndex;

use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::commit_log::CommitLog;
use coderet_store::content_store::ContentStore;
use coderet_store::file_blob_store::FileBlobStore;
use coderet_store::file_store::FileStore;
// use coderet_store::relation_store::RelationStore; // Removed
use coderet_store::storage::Store;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

/// Shared, read-only handle to an indexed repository for agent/tool use.
pub struct RepoContext {
    pub root: PathBuf,
    pub branch: String,
    pub index_dir: PathBuf,
    pub config: Config,
    // pub manager: Arc<IndexManager>, // Removed: Manager is now constructed by the consumer
    pub graph: Arc<RwLock<CodeGraph>>,
    pub file_store: Arc<FileStore>,
    pub content_store: Arc<ContentStore>,
    pub file_blob_store: Arc<FileBlobStore>,
    pub chunk_store: Arc<ChunkStore>,
    // pub relation_store: Arc<RelationStore>, // Removed
    pub commit_log: Option<CommitLog>,
    pub embedder: Option<Arc<dyn coderet_core::traits::Embedder + Send + Sync>>,
    // Indices
    pub lexical: Arc<LexicalIndex>,
    pub vector: Arc<Mutex<VectorIndex>>,
}

impl RepoContext {
    /// Build a context from the current working directory and optional config path.
    pub async fn from_env(config_path: Option<&Path>) -> Result<Self> {
        let root = std::env::current_dir().context("failed to get current directory")?;
        let branch = current_branch();
        let index_dir = root.join(".codeindex").join("branches").join(&branch);
        if !index_dir.exists() {
            return Err(anyhow!(
                "Index not found at {}. Run `coderet index --full` first.",
                index_dir.display()
            ));
        }

        let config = if let Some(path) = config_path {
            Config::from_file(path)?
        } else {
            Config::load()?
        };

        let db_path = index_dir.join("store.db");
        let store = Store::open(&db_path).context("failed to open index store")?;

        let file_store = Arc::new(FileStore::new(store.clone())?);
        let content_store = Arc::new(ContentStore::new(store.clone())?);
        let file_blob_store = Arc::new(FileBlobStore::new(store.clone())?);
        let chunk_store = Arc::new(ChunkStore::new(store.clone())?);
        // let relation_store = Arc::new(RelationStore::new(store.clone())?); // Removed
        
        // Load graph from file
        let graph_path = index_dir.join("graph.bin");
        let graph = Arc::new(RwLock::new(CodeGraph::load(&graph_path)?));
        
        let commit_log = CommitLog::new(store.clone()).ok();

        let lexical = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
        let vector = Arc::new(Mutex::new(
            VectorIndex::new(&index_dir.join("vector.lance")).await?,
        ));



        // Try to initialize embedder using config/environment.
        let embedder = select_embedder(&config.embedding).await.ok();

        Ok(Self {
            root,
            branch,
            index_dir,
            config,
            graph,
            file_store,
            content_store,
            file_blob_store,
            chunk_store,
            // relation_store,
            // relation_store,
            commit_log,
            embedder,
            lexical,
            vector,
        })
    }
}

fn current_branch() -> String {
    if let Ok(out) = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
    {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let trimmed = s.trim();
                if !trimmed.is_empty() && trimmed != "HEAD" {
                    return trimmed.to_string();
                }
            }
        }
    }
    "default".to_string()
}
