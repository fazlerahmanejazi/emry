use anyhow::Result;
use emry_agent::project as agent_context;
use std::path::Path;
use super::ui;

pub async fn handle_status(config_path: Option<&Path>) -> Result<()> {
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let root = ctx.root.clone();
    let config = ctx.config.clone();
    let branch = ctx.branch.clone();
    let index_dir = ctx.index_dir.clone();

    ui::print_key_value("Repository", &root.display().to_string());
    ui::print_key_value("Branch", &branch);
    ui::print_key_value(
        "Config",
        &format!("default_mode={:?}, top_k={}", config.search.mode, config.search.top_k),
    );

    let surreal_exists = index_dir.join("surreal.db").exists();

    ui::print_key_value("Index directory", &index_dir.display().to_string());
    ui::print_key_value(
        " - SurrealDB",
        if surreal_exists { "present" } else { "missing" },
    );

    if let Some(surreal) = &ctx.surreal_store {
        if let Ok(count) = surreal.count_files().await {
             ui::print_key_value("Files tracked", &count.to_string());
        }

        // Show recent commit log entries for lineage
        if let Ok(entries) = surreal.list_commits(5).await {
            if !entries.is_empty() {
                println!("Recent index commits:");
                for entry in entries {
                    println!(" - {} @ {} {}", entry.commit_id, entry.timestamp, entry.note);
                }
            }
        }
    } else {
        println!("SurrealStore not available.");
    }

    Ok(())
}
