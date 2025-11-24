use crate::commands::embedders::select_embedder;
use anyhow::{anyhow, Context, Result};
use coderet_config::Config;
use coderet_graph::graph::CodeGraph;
use coderet_index::lexical::LexicalIndex;
use coderet_index::manager::IndexManager;
use coderet_index::summaries::SummaryIndex as SimpleSummaryIndex;
use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::content_store::ContentStore;
use coderet_store::file_blob_store::FileBlobStore;
use coderet_store::file_store::FileStore;
use coderet_store::relation_store::RelationStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Centralized, read-only access to an indexed repository.
/// This keeps the CLI command handlers from re-building the same state.
pub struct RepoContext {
    pub root: PathBuf,
    pub branch: String,
    pub index_dir: PathBuf,
    pub config: Config,
    pub manager: Arc<IndexManager>,
    pub graph: Arc<CodeGraph>,
    pub content_store: Arc<ContentStore>,
    pub embedder: Option<Arc<dyn coderet_core::traits::Embedder + Send + Sync>>,
}

impl RepoContext {
    pub async fn from_env(config_path: Option<&Path>) -> Result<Self> {
        let root = std::env::current_dir().context("failed to get current directory")?;
        let branch = crate::commands::current_branch();
        let index_dir = root.join(".codeindex").join("branches").join(&branch);
        if !index_dir.exists() {
            return Err(anyhow!(
                "Index not found at {}. Run `coderet index --full` first.",
                index_dir.display()
            ));
        }

        let config = if let Some(p) = config_path {
            Config::from_file(p)?
        } else {
            Config::load()?
        };

        let db_path = index_dir.join("store.db");
        let db = sled::open(&db_path).context("failed to open index store")?;

        let file_store = Arc::new(FileStore::new(db.clone())?);
        let content_store = Arc::new(ContentStore::new(db.clone())?);
        let file_blob_store = Arc::new(FileBlobStore::new(db.clone())?);
        let chunk_store = Arc::new(ChunkStore::new(db.clone())?);
        let relation_store = Arc::new(RelationStore::new(db.clone())?);
        let graph = Arc::new(CodeGraph::new(db.clone())?);

        let lexical = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
        let vector = Arc::new(Mutex::new(
            VectorIndex::new(&index_dir.join("vector.lance")).await?,
        ));

        let embedder = select_embedder(&config.embedding);
        let manager = Arc::new(IndexManager::new(
            lexical,
            vector,
            embedder.clone(),
            file_store,
            chunk_store,
            content_store.clone(),
            file_blob_store,
            relation_store,
            graph.clone(),
            Some(Arc::new(Mutex::new(
                SimpleSummaryIndex::new(&index_dir.join("summaries.db")).await?,
            ))),
        ));

        Ok(Self {
            root,
            branch,
            index_dir,
            config,
            manager,
            graph,
            content_store,
            embedder,
        })
    }
}
