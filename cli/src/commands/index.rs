use anyhow::Result;
use emry_config::Config;
use emry_agent::project::embedder::{select_embedder, get_embedding_dimension};
use emry_core::models::Language;
use emry_core::scanner::scan_repo;

use emry_engine::ingest::pipeline::{compute_hash, FileInput};
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
    let vector_dim = get_embedding_dimension(&config.embedding);
    
    // Initialize SurrealStore
    let surreal_path = index_dir.join("surreal.db");
    let surreal_store = Arc::new(SurrealStore::new(&surreal_path, vector_dim).await?);
    let ingestion_service = IngestionService::new(surreal_store.clone(), embedder_for_manager.clone());

    let spinner_style = ProgressStyle::default_spinner()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        .template("{spinner:.green} {msg}")
        .unwrap();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(spinner_style.clone());
    spinner.set_message("Scanning repository...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let scanned_files = scan_repo(&root, &config.core);
    trace!("Scanned {} files.", scanned_files.len());
    spinner.finish_and_clear();
    println!("Found {} source files to index.", scanned_files.len());

    // Load prior metadata
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

    for (path, _rec) in meta_by_path.iter() {
        if !current_paths.contains(path) {
            surreal_store.delete_file(&path.to_string_lossy()).await?;
            removed_files.push(path.clone());
            stats.removed_files += 1;
        }
    }

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
    
    let pb = ProgressBar::new(num_scanned_files as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message("Reading files");

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

    pb.finish_with_message("File reading complete");

    let pb_proc = ProgressBar::new(read_results.len() as u64);
    pb_proc.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb_proc.set_message("Processing changes");

    let mut work_items: Vec<FileInput> = Vec::new();
    for (_i, fr) in read_results.into_iter().enumerate() {
        pb_proc.inc(1);

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
    pb_proc.finish_with_message("Change detection complete");

    if work_items.is_empty() {
        println!("No new or updated files to index.");
    } else {
        println!("Analyzing source code...");
        let pb_analyze = ProgressBar::new(work_items.len() as u64);
        pb_analyze.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"));
        pb_analyze.set_message("Parsing & Chunking");

        use emry_engine::ingest::pipeline::{analyze_source_files, generate_embeddings};

        pb_analyze.enable_steady_tick(Duration::from_millis(100));
        let mut prepared = analyze_source_files(work_items, &config, concurrency).await;
        pb_analyze.finish_with_message("Analysis complete");

        if let Some(emb) = embedder {
            println!("Generating embeddings...");
            let pb_embed = ProgressBar::new_spinner();
             pb_embed.set_style(ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.green} {msg}")
                .unwrap());
            pb_embed.set_message("Embedding chunks...");
            pb_embed.enable_steady_tick(Duration::from_millis(100));
            
            generate_embeddings(&mut prepared, emb).await;
            
            pb_embed.finish_and_clear();
            println!("Embeddings generated");
        }

        
        use emry_engine::ingest::service::IngestionContext;

        // Create contexts
        let contexts: Vec<IngestionContext> = prepared.into_iter().map(IngestionContext::new).collect();

        let pb_nodes = ProgressBar::new(contexts.len() as u64);
        pb_nodes.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"));
        pb_nodes.set_message("Ingesting nodes");

        for ctx in &contexts {
            if let Err(e) = ingestion_service.ingest_nodes(ctx).await {
                 eprintln!("Failed to ingest nodes for {}: {}", ctx.file.path.display(), e);
            }
            pb_nodes.inc(1);
        }
        pb_nodes.finish_with_message("Nodes ingested");

        let pb_edges = ProgressBar::new(contexts.len() as u64);
        pb_edges.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"));
        pb_edges.set_message("Ingesting edges");

        for ctx in &contexts {
            if let Err(e) = ingestion_service.ingest_edges(ctx).await {
                 eprintln!("Failed to ingest edges for {}: {}", ctx.file.path.display(), e);
            }
            pb_edges.inc(1);
        }
        pb_edges.finish_with_message("Edges ingested");
    }

    let note = format!(
        "Indexed files: new={}, updated={}, removed={}, skipped={}",
        stats.new_files, stats.updated_files, stats.removed_files, stats.skipped_files
    );
    
    surreal_store.add_commit(commit_id, SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0), note).await?;

    use super::ui;
    ui::print_success("Indexing complete!");
    Ok(())
}
