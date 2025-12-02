use super::embedder::{select_embedder, get_embedding_dimension};
use anyhow::{anyhow, Context, Result};
use emry_config::Config;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Shared, read-only handle to an indexed repository for agent/tool use.
pub struct RepoContext {
    pub root: PathBuf,
    pub branch: String,
    pub index_dir: PathBuf,
    pub config: Config,
    pub embedder: Option<Arc<dyn emry_core::traits::Embedder + Send + Sync>>,
    pub surreal_store: Option<Arc<emry_store::SurrealStore>>,
}

impl RepoContext {
    /// Build a context from the current working directory and optional config path.
    pub async fn from_env(config_path: Option<&Path>) -> Result<Self> {
        let root = std::env::current_dir().context("failed to get current directory")?;
        let branch = current_branch();
        let index_dir = root.join(".codeindex").join("branches").join(&branch);
        if !index_dir.exists() {
            return Err(anyhow!(
                "Index not found at {}. Run `emry index --full` first.",
                index_dir.display()
            ));
        }

        let config = if let Some(path) = config_path {
            Config::from_file(path)?
        } else {
            Config::load()?
        };

        // Try to initialize embedder using config/environment.
        let embedder = select_embedder(&config.embedding).await.ok();
        let vector_dim = get_embedding_dimension(&config.embedding);

        // Initialize SurrealStore
        let surreal_path = index_dir.join("surreal.db");
        let surreal_store = emry_store::SurrealStore::new(&surreal_path, vector_dim).await.ok().map(Arc::new);

        Ok(Self {
            root,
            branch,
            index_dir,
            config,
            embedder,
            surreal_store,
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
