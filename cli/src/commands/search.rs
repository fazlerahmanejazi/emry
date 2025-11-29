use anyhow::Result;
use clap::ValueEnum;
use emry_agent::project as agent_context;
use emry_core::models::Language;
use emry_engine::search::service::SearchService;
use std::path::Path;
use std::path::PathBuf;


use super::regex_utils;
use super::utils::{build_single_globset, path_matches};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum CliSearchMode {
    Lexical,
    Semantic,
    Hybrid,
}

pub async fn handle_search(
    query: String,
    config_path: Option<&Path>,
    limit: usize,
    _mode: Option<CliSearchMode>,
    lang: Option<String>,
    path: Option<String>,
    symbol: bool,
    regex: bool,
    no_ignore: bool,
) -> Result<()> {
    println!("Searching for: {}", query);
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let root = ctx.root.clone();
    let config = ctx.config.clone();
    let _embedder = ctx.embedder.clone();
    let embedder = ctx.embedder.clone();
    
    // Initialize SurrealStore & SearchService
    // Initialize SurrealStore & SearchService
    // Reuse store from context if available, or error out
    let surreal_store = ctx.surreal_store.clone()
        .ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized in context"))?;
    let search_service = SearchService::new(surreal_store.clone(), embedder.clone());
    
    // Legacy Manager (kept for now if needed)
    // let manager = Arc::new(IndexManager::new(...));

    // Symbol search short-circuit
    if symbol {
        let matcher = build_single_globset(path.as_deref());
        let lang_filter = lang.as_deref().map(Language::from_name);
        let mut matches = Vec::new();
        if let Ok(nodes) = surreal_store.find_nodes_by_label(&query, None).await {
            for node in nodes {
                // node.label is already filtered by CONTAINS query in find_nodes_by_label
                // but we can keep the check if we want exact match or if the query was broad
                // find_nodes_by_label does "name CONTAINS $label"
                
                let file_path = PathBuf::from(&node.file_path);
                if let Some(lf) = lang_filter.as_ref() {
                    if let Some(ext) = file_path.extension().and_then(|s| s.to_str()) {
                        if Language::from_extension(ext) != *lf {
                            continue;
                        }
                    }
                }
                if !path_matches(&matcher, &root, &file_path) {
                    continue;
                }
                matches.push((node.label.clone(), file_path, node.id.clone()));
            }
        }
        println!("Found {} symbol matches:", matches.len());
        for (i, (name, file_path, id)) in matches.iter().enumerate() {
            println!("{}: {} ({}) - ID: {}", i + 1, name, file_path.display(), id);
        }
        return Ok(())
    }

    // Regex short-circuit
    if regex {
        let matcher = build_single_globset(path.as_deref());
        let lang_filter = lang.as_deref().map(Language::from_name);
        let matches = regex_utils::regex_search(&root, &query, &config.core, !no_ignore)?;
        if matches.is_empty() {
            println!("No matches for regex '{}'.", query);
        } else {
            println!("Regex matches for '{}':", query);
            for (p, line, content) in matches {
                if let Some(lf) = lang_filter.as_ref() {
                    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                        if Language::from_extension(ext) != *lf {
                            continue;
                        }
                    }
                }
                if !path_matches(&matcher, &root, &p) {
                    continue;
                }
                let rel = p.strip_prefix(&root).unwrap_or(&p);
                println!("{}:{}: {}", rel.display(), line, content);
            }
        }
        return Ok(())
    }

    // New Search Logic
    let results = search_service.search(&query, limit).await?;
    
    // Display results
    for (i, chunk) in results.iter().enumerate() {
        println!(
            "\n#{} {}:{}-{}",
            i + 1,
            chunk.file.id.to_string(), // Thing ID
            chunk.start_line,
            chunk.end_line
        );
        // print_snippet requires emry_core::models::Chunk, we have ChunkRecord.
        // We can just print content directly for now.
        println!("{}", chunk.content.trim());
    }

    Ok(())
}