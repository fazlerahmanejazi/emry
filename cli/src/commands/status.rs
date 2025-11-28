use anyhow::Result;
use emry_context as agent_context;
use emry_store::{commit_log::CommitLog, file_store::FileStore};
use std::path::Path;

pub async fn handle_status(config_path: Option<&Path>) -> Result<()> {
    // Fixed: Removed unnecessary nested runtime creation.
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let root = ctx.root.clone();
    let config = ctx.config.clone();
    let branch = ctx.branch.clone();
    let index_dir = ctx.index_dir.clone();

    println!("Repository: {}", root.display());
    println!("Branch: {}", branch);
    println!(
        "Config: default_mode={:?}, top_k={}",
        config.search.mode, config.search.top_k
    );

    let lexical_exists = index_dir.join("lexical").exists();
    let vector_exists = index_dir.join("vector.lance").exists();
    let store_exists = index_dir.join("store.db").exists();

    println!("Index directory: {}", index_dir.display());
    println!(
        " - Lexical index: {}",
        if lexical_exists { "present" } else { "missing" }
    );
    println!(
        " - Vector index: {}",
        if vector_exists { "present" } else { "missing" }
    );
    println!(
        " - Store (sled): {}",
        if store_exists { "present" } else { "missing" }
    );

    if store_exists {
        if let Ok(store) = emry_store::Store::open(&index_dir.join("store.db")) {
            if let Ok(file_store) = FileStore::new(store) {
                if let Ok(files) = file_store.list_metadata() {
                    println!("Files tracked: {}", files.len());
                }
            }
        }
    }

    // Show recent commit log entries for lineage
    if store_exists {
        if let Ok(store) = emry_store::Store::open(&index_dir.join("store.db")) {
            if let Ok(commit_log) = CommitLog::new(store) {
                if let Ok(entries) = commit_log.list(5) {
                    if !entries.is_empty() {
                        println!("Recent index commits:");
                        for entry in entries {
                            println!(" - {} @ {} {}", entry.id, entry.timestamp, entry.note);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
