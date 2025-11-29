use anyhow::Result;
use emry_config::Config;
use emry_agent::project::embedder::select_embedder;
use emry_core::models::Language;
use emry_core::scanner::scan_repo;
use emry_core::stack_graphs::loader::Language as StackGraphLanguage;
use emry_core::stack_graphs::manager::StackGraphManager;
use emry_engine::ingest::pipeline::{compute_hash, prepare_files_async, FileInput, PreparedFile};
use emry_engine::ingest::service::IngestionService;
use emry_store::{SurrealStore, FileRecord};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{info, trace};

use super::utils::current_branch;

#[derive(Default)]
struct IndexStats {
    new_files: usize,
    updated_files: usize,
    removed_files: usize,
    skipped_files: usize,
}

pub async fn handle_index(full: bool, config_path: Option<&Path>) -> Result<()> {
    println!("Indexing repository...");
    let root = std::env::current_dir()?;
    let branch = current_branch();
    let index_dir = root.join(".codeindex").join("branches").join(branch);

    let config = if let Some(p) = config_path {
        Config::from_file(p)?
    } else {
        Config::load()? 
    };

    if index_dir.exists() {
        if full {
            println!("Full rebuild requested; clearing existing index...");
            let _ = std::fs::remove_dir_all(&index_dir);
        } else {
            info!(
                "Incremental indexing: reusing existing index at {}",
                index_dir.display()
            );
        }
    }
    std::fs::create_dir_all(&index_dir)?;

    // Select embedder
    let embedder = select_embedder(&config.embedding).await.ok();
    let embedder_for_manager = embedder.clone();
    
    // Initialize SurrealStore (Main metadata & vector store)
    let surreal_path = index_dir.join("surreal.db");
    let surreal_store = Arc::new(SurrealStore::new(&surreal_path).await?);
    let ingestion_service = IngestionService::new(surreal_store.clone(), embedder_for_manager.clone());

    // Scan files
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Scanning repository...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let scanned_files = scan_repo(&root, &config.core);
    trace!("Scanned {} files.", scanned_files.len());
    spinner.finish_with_message(format!(
        "Found {} source files to index.",
        scanned_files.len()
    ));

    // Load prior metadata for incremental decisions from SurrealDB
    let existing_files = surreal_store.list_files().await?;
    let mut meta_by_path: HashMap<PathBuf, FileRecord> = HashMap::new();
    for f in existing_files {
        meta_by_path.insert(PathBuf::from(&f.path), f);
    }

    let current_paths: HashSet<PathBuf> = scanned_files.iter().map(|f| f.path.clone()).collect();
    let mut stats = IndexStats::default();

    let commit_id = format!(
        "commit:{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let mut removed_files: Vec<PathBuf> = Vec::new();

    // Detect deletions
    for (path, _rec) in meta_by_path.iter() {
        if !current_paths.contains(path) {
            surreal_store.delete_file(&path.to_string_lossy()).await?;
            removed_files.push(path.clone());
            stats.removed_files += 1;
        }
    }

    // Track symbol registry and edges
    // Legacy variables removed
    // let mut symbol_registry: Vec<SymbolRecord> = Vec::new();
    // let mut call_edges: Vec<(String, String)> = Vec::new();
    // let mut import_edges: Vec<(String, String)> = Vec::new();
    
    // let mut work_items: Vec<FileInput> = Vec::new();

    #[derive(Clone)]
    struct FileRead {
        path: PathBuf,
        language: Language,
        content: String,
        hash: String,
        last_modified: u64,
    }

    let concurrency = 8;
    let num_scanned_files = scanned_files.len();
    let read_results: Vec<FileRead> =
        stream::iter(scanned_files.clone().into_iter().map(|file| async move {
            let content = match tokio::fs::read_to_string(&file.path).await {
                Ok(c) => c,
                Err(err) => {
                    eprintln!("Skipping {}: {}", file.path.display(), err);
                    return None;
                }
            };
            let last_modified = tokio::fs::metadata(&file.path)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|ts| ts.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or_else(|| {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                });
            let hash = compute_hash(&content);
            Some(FileRead {
                path: file.path,
                language: file.language,
                content,
                hash,
                last_modified,
            })
        }))
        .buffer_unordered(concurrency)
        .filter_map(|x| async move { x })
        .collect()
        .await;

    let pb = ProgressBar::new(num_scanned_files as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:30.cyan/blue} {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message("Processing files");

    let mut work_items: Vec<FileInput> = Vec::new();
    for (_i, fr) in read_results.into_iter().enumerate() {
        pb.inc(1);

        // Check incremental status
        let prev_meta = meta_by_path.get(&fr.path);
        let file_node_id = format!("file:{}", fr.path.to_string_lossy()); // SurrealDB ID convention we use in IngestService

        if let Some(prev) = prev_meta {
            if prev.hash == fr.hash {
                // No change
                stats.skipped_files += 1;
                continue;
            } else {
                // Changed: delete old version first (clears chunks/symbols)
                surreal_store.delete_file(&fr.path.to_string_lossy()).await?;
                stats.updated_files += 1;
            }
        } else {
            stats.new_files += 1;
        }

        work_items.push(FileInput {
            path: fr.path.clone(),
            language: fr.language.clone(),
            file_id: 0, // Not used with SurrealDB Thing IDs
            file_node_id,
            hash: fr.hash.clone(),
            content: fr.content,
            last_modified: fr.last_modified,
        });
    }
    pb.finish_with_message("File processing complete");

    println!("Generating chunks and embeddings...");
    let prepared: Vec<PreparedFile> =
        prepare_files_async(work_items, &config, embedder.clone(), concurrency).await;

    // --- Stack Graphs Global Sync ---
    println!("Building global stack graph...");
    let stack_graph_path = index_dir.join("stack_graph.bin");
    let mut stack_graph_manager = StackGraphManager::new(stack_graph_path)?;
    
    let mut all_stack_graph_files: Vec<(PathBuf, String, Language, String)> = Vec::new();
    
    // 1. Add prepared files (new/updated)
    for pf in &prepared {
        all_stack_graph_files.push((pf.path.clone(), pf.content.clone(), pf.language.clone(), pf.hash.clone()));
    }
    
    // 2. Add skipped files (fetch content from meta_by_path or disk)
    let prepared_paths: HashSet<&PathBuf> = prepared.iter().map(|p| &p.path).collect();
    
    for file in &scanned_files {
        if !prepared_paths.contains(&file.path) {
            if let Some(meta) = meta_by_path.get(&file.path) {
                 // Use content from SurrealDB record which is already in memory
                 all_stack_graph_files.push((file.path.clone(), meta.content.clone(), file.language.clone(), meta.hash.clone()));
                 continue;
            }
            // Fallback to disk (should rarely happen if meta_by_path is consistent)
            if let Ok(content) = std::fs::read_to_string(&file.path) {
                let hash = compute_hash(&content);
                all_stack_graph_files.push((file.path.clone(), content, file.language.clone(), hash));
            }
        }
    }
    
    let stack_graph_files: Vec<(PathBuf, String, StackGraphLanguage, String)> = all_stack_graph_files
        .into_iter()
        .filter_map(|(path, content, lang, hash)| {
            let sg_lang = match lang {
                Language::Rust => Some(StackGraphLanguage::Rust),
                Language::Python => Some(StackGraphLanguage::Python),
                Language::JavaScript => Some(StackGraphLanguage::JavaScript),
                Language::TypeScript => Some(StackGraphLanguage::TypeScript),
                Language::Java => Some(StackGraphLanguage::Java),
                _ => None,
            };
            sg_lang.map(|l| (path, content, l, hash))
        })
        .collect();

    stack_graph_manager.sync(&stack_graph_files, &root)?;
    
    let global_call_edges = stack_graph_manager.extract_call_edges()?;
    
    // 1. Build Symbol Lookup
    let mut symbol_lookup: HashMap<(String, String), String> = HashMap::new();
    
    // From existing graph (SurrealDB)
    if let Ok(nodes) = surreal_store.list_all_symbols().await {
        for node in nodes {
            symbol_lookup.insert((node.file_path.clone(), node.label.clone()), node.id.to_string());
        }
    }
    // From new/updated files
    for file in &prepared {
        for sym in &file.symbols {
             // ID construction must match IngestionService
             let id = format!("symbol:{}::{}", file.path.to_string_lossy(), sym.name);
             symbol_lookup.insert((file.path.to_string_lossy().to_string(), sym.name.clone()), id);
        }
    }
    
    // 2. Process Global Edges
    for edge in global_call_edges {
        let from_path = PathBuf::from(&edge.from_file);
        // We need to handle file IDs properly. IngestionService uses: ("file", path_string)
        // So the ID string is "file:<path>"
        let source_tuple = ("file".to_string(), from_path.to_string_lossy().to_string());
        
        if let Some(target_id_str) = symbol_lookup.get(&(edge.to_file, edge.to_symbol)) {
            // target_id_str should be "symbol:path::name"
            if let Some(id_part) = target_id_str.strip_prefix("symbol:") {
                let target_tuple = ("symbol".to_string(), id_part.to_string());
                surreal_store.add_graph_edge(source_tuple, target_tuple, "calls").await?;
            }
        }
    }

    // Ingest new/updated files - Two-Pass Strategy
    
    // Pass 1: Ingest Nodes
    println!("Ingesting nodes (Pass 1/2)...");
    let pb_nodes = ProgressBar::new(prepared.len() as u64);
    pb_nodes.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
        .unwrap()
        .progress_chars("#>-"));

    for pf in &prepared {
        if let Err(e) = ingestion_service.ingest_nodes(pf.clone()).await {
             eprintln!("Failed to ingest nodes for {}: {}", pf.path.display(), e);
        }
        pb_nodes.inc(1);
    }
    pb_nodes.finish_with_message("Nodes ingested");

    // Pass 2: Ingest Edges
    println!("Ingesting edges (Pass 2/2)...");
    let pb_edges = ProgressBar::new(prepared.len() as u64);
    pb_edges.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
        .unwrap()
        .progress_chars("#>-"));

    for pf in &prepared {
        if let Err(e) = ingestion_service.ingest_edges(pf.clone()).await {
             eprintln!("Failed to ingest edges for {}: {}", pf.path.display(), e);
        }
        pb_edges.inc(1);
    }
    pb_edges.finish_with_message("Edges ingested");

    // Commit log
    let note = format!(
        "Indexed files: new={}, updated={}, removed={}, skipped={}",
        stats.new_files, stats.updated_files, stats.removed_files, stats.skipped_files
    );
    
    surreal_store.add_commit(commit_id, SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0), note).await?;

    println!("✓ Indexing complete!");
    Ok(())
}
