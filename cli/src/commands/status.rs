use anyhow::Result;
use emry_agent::project as agent_context;
use std::path::Path;

pub async fn handle_status(config_path: Option<&Path>) -> Result<()> {
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

    let surreal_exists = index_dir.join("surreal.db").exists();

    println!("Index directory: {}", index_dir.display());
    println!(
        " - SurrealDB: {}",
        if surreal_exists { "present" } else { "missing" }
    );

    if let Some(surreal) = &ctx.surreal_store {
        if let Ok(count) = surreal.count_files().await {
             println!("Files tracked: {}", count);
        }
        
        // DEBUG: Print one file record
        let files: Vec<emry_store::FileRecord> = surreal.db().query("SELECT * FROM file LIMIT 1").await?.take(0)?;
        if let Some(f) = files.first() {
            println!("DEBUG: Sample File Record: {:?}", f);
        } else {
            println!("DEBUG: No file records found!");
        }

        // Debug: List all files
        let all_files: Vec<emry_store::FileRecord> = surreal.db().query("SELECT * FROM file").await?.take(0)?;
        println!("Total files: {}", all_files.len());
        for f in &all_files {
            if f.path.contains("collision_test") {
                println!("Found collision_test file: {} ID: {:?}", f.path, f.id);
            }
        }
        
        // Debug: Check symbols for collision_test files
        let symbols: Vec<serde_json::Value> = surreal.db().query("SELECT type::string(id) as id, name, file.path as file_path FROM symbol WHERE string::contains(type::string(file), 'collision_test')").await?.take(0)?;
        println!("Symbols in collision_test: {}", symbols.len());
        for s in &symbols {
            println!("  Symbol: {:?}", s);
        }
        
        // Debug: Check all main symbols
        let all_mains: Vec<serde_json::Value> = surreal.db().query("SELECT type::string(id) as id, name, file.path as file_path FROM symbol WHERE name = 'main'").await?.take(0)?;
        println!("All main symbols: {}", all_mains.len());
        for m in &all_mains {
            println!(" Main: {:?}", m);
        }
        
        // Debug: Check for call edges from any main symbol
        let main_edges: Vec<serde_json::Value> = surreal.db().query("SELECT * FROM calls WHERE string::contains(type::string(in), '::main')").await?.take(0)?;
        println!("Call edges from any main: {}", main_edges.len());
        for e in &main_edges {
            println!("  Edge: {:?}", e);
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
