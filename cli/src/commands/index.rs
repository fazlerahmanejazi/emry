use anyhow::Result;
use emry_config::Config;
use emry_context::embedder::select_embedder;
use emry_core::models::Language;
use emry_core::scanner::scan_repo;
use emry_core::stack_graphs::loader::Language as StackGraphLanguage;
use emry_core::stack_graphs::manager::StackGraphManager;
use emry_graph::graph::{CodeGraph, GraphNode};
use emry_index::lexical::LexicalIndex;
use emry_index::vector::VectorIndex;
use emry_pipeline::index::{compute_hash, prepare_files_async, FileInput, PreparedFile};
use emry_pipeline::manager::IndexManager;
use emry_store::chunk_store::ChunkStore;
use emry_store::commit_log::{CommitEntry, CommitLog};
use emry_store::content_store::ContentStore;
use emry_store::file_store::{FileMetadata, FileStore};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
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

    // Initialize storage
    let db_path = index_dir.join("store.db");
    let store = emry_store::Store::open(&db_path)?;

    let file_store = Arc::new(FileStore::new(store.clone())?);
    let content_store = Arc::new(ContentStore::new(store.clone())?);
    let file_blob_store = Arc::new(emry_store::file_blob_store::FileBlobStore::new(
        store.clone(),
    )?);
    let chunk_store = Arc::new(ChunkStore::new(store.clone())?);
    let commit_log = Arc::new(CommitLog::new(store.clone())?);
    
    let graph_path = index_dir.join("graph.bin");
    let graph = Arc::new(RwLock::new(CodeGraph::load(&graph_path)?));

    // Initialize indices
    let lexical = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    let vector = Arc::new(Mutex::new(
        VectorIndex::new(&index_dir.join("vector.lance")).await?,
    ));

    // Select embedder (optional; enables vector search if available)
    let embedder = select_embedder(&config.embedding).await.ok();
    let embedder_for_manager = embedder.clone();

    let graph_arc = graph.clone();
    let manager = Arc::new(IndexManager::new(
        lexical.clone(),
        vector.clone(),
        embedder_for_manager,
        file_store.clone(),
        chunk_store.clone(),
        content_store.clone(),
        file_blob_store.clone(),
        graph_arc.clone(),
    ));

    // Scan files using config include/exclude globs and language detection.
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
    trace!("Scanned {} files. Paths:", scanned_files.len());
    for file in &scanned_files {
        trace!("  - {}", file.path.display());
    }
    spinner.finish_with_message(format!(
        "Found {} source files to index.",
        scanned_files.len()
    ));

    // Load prior metadata for incremental decisions
    let existing_meta = file_store.list_metadata()?;
    let mut meta_by_path: HashMap<PathBuf, FileMetadata> = HashMap::new();
    for m in existing_meta {
        meta_by_path.insert(m.path.clone(), m);
    }

    let current_paths: HashSet<PathBuf> = scanned_files.iter().map(|f| f.path.clone()).collect();
    let mut stats = IndexStats::default();

    // Begin transaction for new/updated files
    let mut txn = manager.begin_transaction().await?;
    let commit_id = format!(
        "commit:{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let mut removed_files: Vec<PathBuf> = Vec::new();

    // Detect deletions (present before, missing now)
    let mut stale_chunk_ids: Vec<String> = Vec::new();
    for (path, meta) in meta_by_path.iter() {
        if !current_paths.contains(path) {
            let ids = chunk_store.get_chunks_for_file(meta.id)?;
            stale_chunk_ids.extend(ids);
            let _file_node_id = format!("file:{}", meta.id);
            txn.delete_file_node(path.to_string_lossy().to_string());
            file_store.delete_file(path)?;
            removed_files.push(path.clone());
            stats.removed_files += 1;
        }
    }
    if !stale_chunk_ids.is_empty() {
        txn.delete_chunks(stale_chunk_ids);
    }

    // Track symbol registry and call/import candidates
    let mut symbol_registry: Vec<emry_core::models::Symbol> = Vec::new();
    let mut call_edges: Vec<(String, String)> = Vec::new(); // (caller_file_node, callee_name)
    let mut import_edges: Vec<(String, String)> = Vec::new(); // (file_node, import_name)

    let mut work_items: Vec<FileInput> = Vec::new();

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

    for (_i, fr) in read_results.into_iter().enumerate() {
        pb.inc(1);

        let (file_id, prev_meta) = match meta_by_path.get(&fr.path) {
            Some(prev) => (prev.id, Some(prev.clone())),
            None => (
                file_store.get_or_create_file_id(fr.path.as_path(), &fr.hash)?,
                None,
            ),
        };
        let file_node_id = format!("file:{}", file_id);

        if let Some(prev) = prev_meta {
            if prev.content_hash == fr.hash {
                let metadata = FileMetadata {
                    id: prev.id,
                    path: fr.path.clone(),
                    last_modified: fr.last_modified,
                    content_hash: prev.content_hash.clone(),
                    last_indexed_run: Some(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    ),
                };
                txn.update_file_metadata(metadata);
                stats.skipped_files += 1;
                continue;
            } else {
                let chunk_ids = chunk_store.get_chunks_for_file(prev.id)?;
                if !chunk_ids.is_empty() {
                    txn.delete_chunks(chunk_ids);
                }
                txn.delete_file_node(fr.path.to_string_lossy().to_string());
                removed_files.push(fr.path.clone());
                stats.updated_files += 1;
            }
        } else {
            stats.new_files += 1;
        }

        work_items.push(FileInput {
            path: fr.path.clone(),
            language: fr.language.clone(),
            file_id,
            file_node_id,
            hash: fr.hash.clone(),
            content: fr.content,
            last_modified: fr.last_modified,
        });
    }
    pb.finish_with_message("File processing complete");

    // Process heavy work (chunking, embedding, symbol extraction) in parallel.
    println!("Generating chunks and embeddings...");
    let prepared: Vec<PreparedFile> =
        prepare_files_async(work_items, &config, embedder.clone(), concurrency).await;

    // --- Stack Graphs Global Sync ---
    println!("Building global stack graph...");
    let stack_graph_path = index_dir.join("stack_graph.bin");
    let mut stack_graph_manager = StackGraphManager::new(stack_graph_path)?;
    
    // Collect content for all files to ensure complete graph resolution.
    // Current sync implementation rebuilds the graph, so we need all files,
    // including those skipped by incremental indexing.
    let mut all_stack_graph_files: Vec<(PathBuf, String, Language, String)> = Vec::new();
    
    // 1. Add prepared files (new/updated)
    for pf in &prepared {
        all_stack_graph_files.push((pf.path.clone(), pf.content.clone(), pf.language.clone(), pf.hash.clone()));
    }
    
    // 2. Add skipped files (fetch content from store or disk)
    let prepared_paths: HashSet<&PathBuf> = prepared.iter().map(|p| &p.path).collect();
    
    for file in &scanned_files {
        if !prepared_paths.contains(&file.path) {
            if let Some(meta) = meta_by_path.get(&file.path) {
                 if let Ok(Some(content)) = content_store.get(&meta.content_hash) {
                     all_stack_graph_files.push((file.path.clone(), content, file.language.clone(), meta.content_hash.clone()));
                     continue;
                 }
            }
            if let Ok(content) = std::fs::read_to_string(&file.path) {
                let hash = compute_hash(&content);
                all_stack_graph_files.push((file.path.clone(), content, file.language.clone(), hash));
            }
        }
    }
    
    // Convert to StackGraphLanguage and filter supported languages
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

    // Sync global graph
    stack_graph_manager.sync(&stack_graph_files, &root)?;
    
    // Extract global edges
    let global_call_edges = stack_graph_manager.extract_call_edges()?;
    
    // Process global call edges from stack-graphs.
    // We need to resolve the (from_file, to_symbol, to_file) triple from stack-graphs
    // into our internal graph node IDs.
    //
    // Strategy:
    // 1. Build a precise lookup: `Map<(Path, Name), SymbolID>` from both existing graph and new symbols.
    // 2. Iterate `global_call_edges` and resolve source and target IDs.
    // 3. Add edges directly to the transaction.

    // 1. Build Symbol Lookup
    let mut symbol_lookup: HashMap<(String, String), String> = HashMap::new(); // (path, name) -> id
    
    // From existing graph
    if let Ok(nodes) = graph.read().unwrap().list_symbols() {
        for node in nodes {
            symbol_lookup.insert((node.file_path.clone(), node.label.clone()), node.id);
        }
    }
    // From new/updated files (overwrites existing if any)
    for sym in &symbol_registry {
        symbol_lookup.insert((sym.file_path.to_string_lossy().to_string(), sym.name.clone()), sym.id.clone());
    }
    
    // 2. Process Global Edges
    for edge in global_call_edges {
        // Resolve Source
        // We need the file_node_id for 'edge.from_file'.
        // We can look it up from file_store or just construct "file:{id}" if we had ID.
        // We have 'meta_by_path'.
        let from_path = PathBuf::from(&edge.from_file);
        let source_id = if let Some(meta) = meta_by_path.get(&from_path) {
             format!("file:{}", meta.id)
        } else {
             // Might be a new file not in meta_by_path yet?
             // But 'prepared' files are new.
             // We can look up in 'prepared' list?
             if let Some(pf) = prepared.iter().find(|p| p.path == from_path) {
                 pf.file_node_id.clone()
             } else {
                 continue; // Should not happen if sync is correct
             }
        };
        
        // Resolve Target
        if let Some(target_id) = symbol_lookup.get(&(edge.to_file, edge.to_symbol)) {
            txn.add_graph_edge(source_id, target_id.clone(), "calls".to_string());
        }
    }
    for pf in prepared {
        let file_node_id = pf.file_node_id.clone();
        txn.add_graph_node(GraphNode {
            id: file_node_id.clone(),
            kind: "file".to_string(),
            label: pf.path.to_string_lossy().to_string(),
            canonical_id: Some(file_node_id.clone()),
            file_path: pf.path.to_string_lossy().to_string(),
        });

        txn.put_file_blob(pf.path.clone(), pf.content.clone());

        for chunk in &pf.chunks {
            txn.put_content(chunk.content_hash.clone(), chunk.content.clone());
            txn.add_graph_node(GraphNode {
                id: chunk.id.clone(),
                kind: "chunk".to_string(),
                label: format!(
                    "{}:{}-{}",
                    chunk.file_path.display(),
                    chunk.start_line,
                    chunk.end_line
                ),
                canonical_id: Some(chunk.id.clone()),
                file_path: chunk.file_path.to_string_lossy().to_string(),
            });
            txn.add_graph_edge(
                file_node_id.clone(),
                chunk.id.clone(),
                "contains".to_string(),
            );
            txn.add_chunk(chunk.clone(), pf.file_id)?;
        }

        let metadata = FileMetadata {
            id: pf.file_id,
            path: pf.path.clone(),
            last_modified: pf.last_modified,
            content_hash: pf.hash.clone(),
            last_indexed_run: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            ),
        };
        txn.update_file_metadata(metadata);

        for sym in &pf.symbols {
            txn.add_graph_node(GraphNode {
                id: sym.id.clone(),
                kind: "symbol".to_string(),
                label: sym.name.clone(),
                canonical_id: Some(sym.id.clone()),
                file_path: sym.file_path.to_string_lossy().to_string(),
            });
            txn.add_graph_edge(file_node_id.clone(), sym.id.clone(), "defines".to_string());
            symbol_registry.push(sym.clone());

            // Hierarchy Restoration: Check if symbol has a parent (e.g., "Class.method")
            // We assume "." or "::" separator.
            if let Some((parent_name, _)) = sym.name.rsplit_once('.')
                .or_else(|| sym.name.rsplit_once("::")) 
            {
                 // We need to find the parent symbol ID.
                 // Since we are iterating, the parent might be in the same file or another.
                 // For same-file parents, we can check pf.symbols.
                 // For cross-file, we rely on symbol_registry (which is incomplete here).
                 // Best effort: Check pf.symbols first.
                 if let Some(parent) = pf.symbols.iter().find(|s| s.name == parent_name) {
                     txn.add_graph_edge(parent.id.clone(), sym.id.clone(), "contains".to_string());
                 }
            }
        }
        for (chunk_id, sym_id) in &pf.chunk_symbol_edges {
            txn.add_graph_edge(
                chunk_id.to_string(),
                sym_id.to_string(),
                "defines".to_string(),
            );
        }

        call_edges.extend(pf.call_edges.clone().into_iter()); // Clone here
        import_edges.extend(pf.import_edges.into_iter());


        trace!("File: {}, Raw call_edges from prepare_file:", pf.path.display());
        for (caller, callee_name) in &pf.call_edges {
            trace!("  - Caller: {}, Callee: {}", caller, callee_name);
        }
    }

    // Resolve call/import edges to known symbols before commit to keep relations transactional.
    let mut name_to_symbol: HashMap<String, String> = HashMap::new();
    for sym in &symbol_registry {
        name_to_symbol
            .entry(sym.name.clone())
            .or_insert(sym.id.clone());
        if let Some(last) = sym.name.rsplit('.').next() {
            name_to_symbol
            .entry(last.to_string())
            .or_insert(sym.id.clone());
        }
    }
    for (caller, callee_name) in call_edges {
        let target = name_to_symbol.get(&callee_name).or_else(|| {
            callee_name
                .rsplit(['.', ':', '/'])
                .next()
                .and_then(|n| name_to_symbol.get(n))
        });
        trace!(
            "Processing call: caller='{}', callee_name='{}', resolved_target_id='{}'",
            caller,
            callee_name,
            target.map_or("None".to_string(), |s| s.clone())
        );
        if let Some(target_id) = target {
            trace!("Adding call edge: {} -> {}", caller, target_id);
            txn.add_graph_edge(caller.clone(), target_id.clone(), "calls".to_string());
        } else {
            trace!("No target found for callee_name='{}'", callee_name);
        }
    }
    for (file_node, import_name) in import_edges {
        let target = name_to_symbol.get(&import_name).or_else(|| {
            import_name
                .rsplit(['.', ':', '/'])
                .next()
                .and_then(|n| name_to_symbol.get(n))
        });
        if let Some(target_id) = target {
            txn.add_graph_edge(file_node.clone(), target_id.clone(), "imports".to_string());
        }
    }

    // Commit transaction
    println!("Committing chunks and symbols...");
    txn.commit().await?;

    // Record commit entry for basic lineage
    let note = format!(
        "Indexed files: new={}, updated={}, removed={}, skipped={}",
        stats.new_files, stats.updated_files, stats.removed_files, stats.skipped_files
    );
    let _ = commit_log.append(CommitEntry {
        id: commit_id,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        note,
    });

    println!("✓ Indexing complete!");
    Ok(())
}
