// Simplified CLI and command handlers using the new architecture.
// Focused on correctness and robustness; supports incremental updates with hash-based detection.
mod context;
mod embedders;
mod index_pipeline;
mod llm;
mod print_snippet;
mod regex_utils;
mod summaries;

use crate::commands::index_pipeline::{compute_hash, prepare_files_async, FileInput, PreparedFile};
use crate::commands::llm::LlmClient;
use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use coderet_agent::{agent::AskAgent, context as agent_context};
use coderet_config::Config;
use coderet_core::models::Language;
use coderet_core::scanner::scan_repo;
use coderet_graph::graph::{CodeGraph, GraphNode};
use coderet_index::lexical::LexicalIndex;
use coderet_index::manager::IndexManager;
use coderet_index::summaries::SummaryIndex as SimpleSummaryIndex;
use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::commit_log::{CommitEntry, CommitLog};
use coderet_store::content_store::ContentStore;
use coderet_store::file_store::{FileMetadata, FileStore};
use coderet_store::relation_store::{RelationStore, RelationType};
use embedders::select_embedder;
use globset::{Glob, GlobSet, GlobSetBuilder};
use indicatif::{ProgressBar, ProgressStyle};
use print_snippet::print_snippet;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use summaries::maybe_generate_summaries;
use termimad::{FmtText, MadSkin};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "code-retriever")]
#[command(about = "A local code retrieval tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Index the current repository
    Index {
        /// Force a full rebuild
        #[arg(long)]
        full: bool,
    },
    /// Search the index
    Search {
        /// The query string
        query: String,

        /// Number of results
        #[arg(long, default_value_t = 10)]
        top: usize,

        /// Search mode
        #[arg(long, value_enum)]
        mode: Option<CliSearchMode>,

        /// Filter by language
        #[arg(long)]
        lang: Option<String>,

        /// Filter by path glob
        #[arg(long)]
        path: Option<String>,

        /// Search for symbol definitions (name match)
        #[arg(long)]
        symbol: bool,

        /// Search summaries instead of code
        #[arg(long)]
        summary: bool,

        /// Treat query as regex (lexical/grep-style)
        #[arg(long)]
        regex: bool,

        /// Do not apply ignore rules (gitignore/config) for regex/grep search
        #[arg(long, default_value_t = false)]
        no_ignore: bool,
    },
    /// Ask (new stack, simplified)
    Ask {
        /// The question
        query: String,
        #[arg(long, value_enum, default_value_t = AskDepth::Default)]
        depth: AskDepth,
        /// Use the agent (plan/execute). Defaults to on; disable with --agent=false.
        #[arg(long, default_value_t = true, action = ArgAction::Set)]
        agent: bool,
        /// Output detail level
        #[arg(long, value_enum, default_value_t = OutputLevel::Progress)]
        output: OutputLevel,
        /// Output JSON (answer, plan, observations)
        #[arg(long, default_value_t = false)]
        json: bool,
        /// Max results per tool call (override default)
        #[arg(long)]
        max_per_step: Option<usize>,
        /// Max observations sent to the LLM (override default)
        #[arg(long)]
        max_observations: Option<usize>,
        /// Max tokens for LLM calls (override default)
        #[arg(long)]
        max_tokens: Option<u32>,
    },
    /// Show status (not yet implemented)
    Status,
    /// Launch the TUI (still legacy)
    Tui,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum AskDepth {
    Shallow,
    Default,
    Deep,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum OutputLevel {
    Silent,
    Progress,
    Detailed,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum CliSearchMode {
    Lexical,
    Semantic,
    Hybrid,
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
    let db = sled::open(&db_path)?;

    let file_store = Arc::new(FileStore::new(db.clone())?);
    let content_store = Arc::new(ContentStore::new(db.clone())?);
    let file_blob_store = Arc::new(coderet_store::file_blob_store::FileBlobStore::new(
        db.clone(),
    )?);
    let chunk_store = Arc::new(ChunkStore::new(db.clone())?);
    let relation_store = Arc::new(RelationStore::new(db.clone())?);
    let commit_log = Arc::new(CommitLog::new(db.clone())?);
    let graph = Arc::new(CodeGraph::new(db.clone())?);

    // Initialize indices
    let lexical = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    let vector = Arc::new(Mutex::new(
        VectorIndex::new(&index_dir.join("vector.lance")).await?,
    ));
    let summary_index = Arc::new(Mutex::new(
        SimpleSummaryIndex::new(&index_dir.join("summaries.db")).await?,
    ));

    // Select embedder (optional; enables vector search if available)
    let embedder = select_embedder(&config.embedding);
    let embedder_for_manager = embedder.clone();
    let _embedder_for_search = embedder.clone();

    let relation_store_arc = relation_store.clone();
    let graph_arc = graph.clone();
    let manager = Arc::new(IndexManager::new(
        lexical.clone(),
        vector.clone(),
        embedder_for_manager,
        file_store.clone(),
        chunk_store.clone(),
        content_store.clone(),
        file_blob_store.clone(),
        relation_store_arc.clone(),
        graph_arc.clone(),
        Some(summary_index.clone()),
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
            let file_node_id = format!("file:{}", meta.id);
            txn.delete_file_node(path.to_string_lossy().to_string());
            let _ = relation_store_arc.delete_by_source(&file_node_id);
            file_store.delete_file(path)?;
            removed_files.push(path.clone());
            stats.removed_files += 1;
        }
    }
    if !stale_chunk_ids.is_empty() {
        txn.delete_chunks(stale_chunk_ids);
    }

    // Track symbol registry and call/import candidates
    let mut symbol_registry: Vec<coderet_core::models::Symbol> = Vec::new();
    let mut call_edges: Vec<(String, String)> = Vec::new(); // (caller_file_node, callee_name)
    let mut import_edges: Vec<(String, String)> = Vec::new(); // (file_node, import_name)
    let mut file_content_map: HashMap<PathBuf, String> = HashMap::new();
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
    use futures::stream::{self, StreamExt};
    let num_scanned_files = scanned_files.len();
    let read_results: Vec<FileRead> =
        stream::iter(scanned_files.into_iter().map(|file| async move {
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

    for (i, fr) in read_results.into_iter().enumerate() {
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
                let _ = relation_store_arc.delete_by_source(&file_node_id);
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

    // Apply prepared results sequentially to preserve transaction integrity.
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
        }
        for (chunk_id, sym_id) in &pf.chunk_symbol_edges {
            txn.add_relation(
                chunk_id.to_string(),
                sym_id.to_string(),
                RelationType::Defines,
            );
            txn.add_graph_edge(
                chunk_id.to_string(),
                sym_id.to_string(),
                "defines".to_string(),
            );
        }

        call_edges.extend(pf.call_edges.into_iter());
        import_edges.extend(pf.import_edges.into_iter());
        if config.summary.enabled {
            file_content_map.insert(pf.path.clone(), pf.content.clone());
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
        if let Some(target_id) = target {
            txn.add_graph_edge(caller.clone(), target_id.clone(), "calls".to_string());
            txn.add_relation(caller.clone(), target_id.clone(), RelationType::Calls);
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
            txn.add_relation(file_node.clone(), target_id.clone(), RelationType::Imports);
        }
    }

    // Commit transaction
    println!("Committing chunks and symbols...");
    txn.commit().await?;

    println!("Generating summaries...");

    let mut summary_guard = summary_index.lock().await;
    maybe_generate_summaries(
        &config,
        &mut *summary_guard,
        embedder.as_ref(),
        &file_store,
        &root,
        &symbol_registry,
        &file_content_map,
        &removed_files,
    )
    .await?;

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

pub async fn handle_search(
    query: String,
    config_path: Option<&Path>,
    limit: usize,
    _mode: Option<CliSearchMode>,
    lang: Option<String>,
    path: Option<String>,
    symbol: bool,
    summary: bool,
    regex: bool,
    no_ignore: bool,
) -> Result<()> {
    println!("Searching for: {}", query);
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let root = ctx.root.clone();
    let config = ctx.config.clone();
    let graph_arc = ctx.graph.clone();
    let content_store = ctx.content_store.clone();
    let index_dir = ctx.index_dir.clone();
    let embedder = ctx.embedder.clone();
    let manager = ctx.manager.clone();

    // Symbol search short-circuit
    if symbol {
        let matcher = build_single_globset(path.as_deref());
        let lang_filter = lang.as_deref().map(Language::from_name);
        let mut matches = Vec::new();
        if let Ok(nodes) = graph_arc.list_symbols() {
            for node in nodes {
                if !node.label.contains(&query) {
                    continue;
                }
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
                matches.push((node.label.clone(), file_path));
            }
        }
        println!("Found {} symbol matches:", matches.len());
        for (i, (name, file_path)) in matches.iter().enumerate() {
            println!("{}: {} ({})", i + 1, name, file_path.display());
        }
        return Ok(());
    }

    if summary {
        let sidx = SimpleSummaryIndex::new(&index_dir.join("summaries.db")).await?;
        if let Some(embedder) = embedder.as_ref() {
            match sidx.semantic_search(&query, embedder.as_ref(), limit).await {
                Ok(matches) => {
                    println!("Found {} summary matches:", matches.len());
                    for (i, (score, sum)) in matches.iter().enumerate() {
                        let loc = sum
                            .file_path
                            .as_ref()
                            .map(|p| {
                                format!(
                                    "{}:{}-{}",
                                    p.display(),
                                    sum.start_line.unwrap_or(0),
                                    sum.end_line.unwrap_or(0)
                                )
                            })
                            .unwrap_or_else(|| sum.target_id.clone());
                        println!("\n#{} (score {:.3}) {}", i + 1, score, loc);
                        println!("{}", sum.text);
                    }
                    return Ok(());
                }
                Err(e) => eprintln!("Semantic summary search failed: {}", e),
            }
        }

        let matches = sidx.search(&query, limit).await?;
        println!("Found {} summary matches:", matches.len());
        for (i, sum) in matches.iter().enumerate() {
            let loc = sum
                .file_path
                .as_ref()
                .map(|p| {
                    format!(
                        "{}:{}-{}",
                        p.display(),
                        sum.start_line.unwrap_or(0),
                        sum.end_line.unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| sum.target_id.clone());
            println!("\n#{} {}", i + 1, loc);
            println!("{}", sum.text);
        }
        return Ok(());
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
        return Ok(());
    }

    let results = manager
        .search_ranked(&query, limit, Some(rank_cfg(&config)))
        .await?;

    let lang_filter = lang.as_deref().map(Language::from_name);
    let matcher = build_single_globset(path.as_deref());
    let mut filtered = results;
    filtered.retain(|hit| {
        let chunk = &hit.chunk;
        let lang_ok = lang_filter
            .as_ref()
            .map(|l| chunk.language == *l)
            .unwrap_or(true);
        let path_ok = path_matches(&matcher, &root, &chunk.file_path);
        lang_ok && path_ok
    });

    // Boost scores if summaries match query
    let mut summary_hits_map: HashMap<String, f32> = HashMap::new();
    let summary_boost = config.ranking.summary;
    if summary_boost > 0.0 {
        let sidx = SimpleSummaryIndex::new(&index_dir.join("summaries.db")).await?;
        if let Some(embedder) = embedder.as_ref() {
            if let Ok(matches) = sidx.semantic_search(&query, embedder.as_ref(), 50).await {
                for (score, sum) in matches {
                    summary_hits_map.insert(sum.target_id.clone(), score);
                }
            }
        } else {
            if let Ok(summaries) = sidx.search(&query, 50).await {
                for sum in summaries {
                    summary_hits_map.insert(sum.target_id.clone(), 1.0);
                }
            }
        }
        for hit in filtered.iter_mut() {
            let mut summary_score: f32 = 0.0;
            if let Some(b) = summary_hits_map.get(&hit.chunk.id) {
                summary_score = summary_score.max(*b);
            }
            let file_key = hit.chunk.file_path.to_string_lossy().to_string();
            if let Some(b) = summary_hits_map.get(&file_key) {
                summary_score = summary_score.max(*b);
            }
            hit.summary_score = Some(summary_score);
        }
    }

    // Learning-to-rank style fusion: normalize components then combine with weights.
    let fused = fuse_scores(filtered, &config, summary_boost);

    // Display results
    for (i, hit) in fused.iter().enumerate() {
        let chunk = &hit.chunk;
        println!(
            "\n#{} [{:.3}] lex={:.3?} vec={:.3?} graph={:.3?} sym={:.3?} sum={:.3?} {}:{}-{}",
            i + 1,
            hit.score,
            hit.lexical_score,
            hit.vector_score,
            hit.graph_boost,
            hit.symbol_boost,
            hit.summary_score,
            chunk.file_path.display(),
            chunk.start_line,
            chunk.end_line
        );
        if let Some(path) = &hit.graph_path {
            println!("Path: {}", path.join(" | "));
        }
        print_snippet(chunk, &root, 2, Some(content_store.as_ref()));
    }

    Ok(())
}

pub async fn handle_ask(
    query: String,
    depth: AskDepth,
    use_agent: bool,
    output: OutputLevel,
    json: bool,
    max_per_step: Option<usize>,
    max_observations: Option<usize>,
    max_tokens: Option<u32>,
    config_path: Option<&Path>,
) -> Result<()> {
    if matches!(output, OutputLevel::Detailed | OutputLevel::Progress) {
        println!("🔎 Query: {}", query);
        if use_agent {
            println!(""); // Blank line for spacing
        }
    }

    // If agent is requested but cannot be initialized (missing LLM keys), fall back to RAG.
    let agent_ctx = match agent_context::RepoContext::from_env(config_path).await {
        Ok(ctx) => Arc::new(ctx),
        Err(e) => {
            if use_agent {
                eprintln!("Agent context unavailable ({}); falling back to RAG.", e);
            }
            return answer_with_rag(&query, depth, output, json, max_tokens, config_path).await;
        }
    };

    let limit = match depth {
        AskDepth::Shallow => 3,
        AskDepth::Default => agent_ctx.config.search.top_k,
        AskDepth::Deep => std::cmp::max(20, agent_ctx.config.search.top_k),
    };

    if !use_agent {
        return answer_with_rag(&query, depth, output, json, max_tokens, config_path).await;
    }

    let mut agent = match AskAgent::new(agent_ctx.clone()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Agent unavailable ({}); falling back to RAG.", e);
            return answer_with_rag(&query, depth, output, json, max_tokens, config_path).await;
        }
    };

    // Apply user overrides
    if let Some(v) = max_per_step {
        agent.config.max_per_step = v;
    }
    if let Some(v) = max_observations {
        agent.config.max_observations = v;
    }
    if let Some(v) = max_tokens {
        agent.config.max_tokens = v;
    }

    let show_progress = matches!(output, OutputLevel::Progress | OutputLevel::Detailed);
    let result = agent.answer_question(&query, limit, show_progress).await?;

    if json {
        let payload = serde_json::json!({
            "answer": result.answer,
            "plan": result.plan,
            "observations": result.observations,
            "actions": result.actions_run,
            "coverage": result.coverage,
            "coverage_notes": result.coverage_notes,
            "coverage_summary": result.coverage_summary,
            "classified": {
                "intent": format!("{:?}", result.intent),
                "secondary_intents": result
                    .classified
                    .secondary_intents
                    .iter()
                    .map(|i| format!("{:?}", i))
                    .collect::<Vec<_>>(),
                "keywords": result.classified.domain_keywords,
            }
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!(
        "\nAnswer:\n{}\n",
        render_markdown_answer(result.answer.trim())
    );

    if matches!(output, OutputLevel::Detailed) {
        println!("Plan steps:");
        for (idx, step) in result.plan.steps.iter().enumerate() {
            println!(" {}. {} {}", idx + 1, step.action, step.params.to_string());
        }

        println!("\nObservations used:");
        for (i, obs) in result.observations.iter().enumerate() {
            println!("\n[{}] {} - {}", i + 1, obs.step_id, obs.action);
            if let Some(err) = &obs.error {
                println!("  error: {}", err);
                continue;
            }
            for ev in &obs.evidence {
                let loc = ev
                    .file_path
                    .as_ref()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "n/a".to_string());
                let range = match (ev.start_line, ev.end_line) {
                    (Some(s), Some(e)) => format!("{}-{}", s, e),
                    _ => "n/a".to_string(),
                };
                println!("  [{}:{}] {}", loc, range, ev.source);
                if !ev.tags.is_empty() {
                    println!("    tags: {}", ev.tags.join(", "));
                }
                println!("{}\n", ev.text);
            }
        }

        println!("\nCoverage summary:");
        println!(
            " search_hits={}, summary_hits={}, symbol_hits={}, graph_hits={}, file_reads={}",
            result.coverage_summary.search_hits,
            result.coverage_summary.summary_hits,
            result.coverage_summary.symbol_hits,
            result.coverage_summary.graph_hits,
            result.coverage_summary.file_reads,
        );
        if !result.coverage_summary.search_queries.is_empty() {
            println!(
                " search queries: {}",
                result.coverage_summary.search_queries.join(" | ")
            );
        }
        if !result.coverage_summary.summary_queries.is_empty() {
            println!(
                " summary queries: {}",
                result.coverage_summary.summary_queries.join(" | ")
            );
        }
        if !result.coverage_summary.symbol_queries.is_empty() {
            println!(
                " symbol queries: {}",
                result.coverage_summary.symbol_queries.join(" | ")
            );
        }
        if !result.coverage_summary.graph_nodes.is_empty() {
            println!(
                " graph roots: {}",
                result.coverage_summary.graph_nodes.join(" | ")
            );
        }
        if !result.coverage_summary.scanned_dirs.is_empty() {
            println!(
                " scanned dirs: {}",
                result.coverage_summary.scanned_dirs.join(" | ")
            );
        }
    }

    Ok(())
}

async fn answer_with_rag(
    query: &str,
    depth: AskDepth,
    output: OutputLevel,
    json: bool,
    max_tokens: Option<u32>,
    config_path: Option<&Path>,
) -> Result<()> {
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let limit = match depth {
        AskDepth::Shallow => 6,
        AskDepth::Default => ctx.config.search.top_k.max(8),
        AskDepth::Deep => std::cmp::max(16, ctx.config.search.top_k * 2),
    };

    let hits = ctx
        .manager
        .search_ranked(query, limit, Some(rank_cfg(&ctx.config)))
        .await?;

    // Deduplicate by file to improve diversity.
    let mut seen = std::collections::HashSet::new();
    let mut snippets = Vec::new();
    for hit in hits {
        let file_key = hit.chunk.file_path.to_string_lossy().to_string();
        if !seen.insert(file_key.clone()) {
            continue;
        }
        let snippet = hit
            .chunk
            .content
            .lines()
            .take(80)
            .collect::<Vec<_>>()
            .join("\n");
        snippets.push((file_key, hit.chunk.start_line, hit.chunk.end_line, snippet));
        if snippets.len() >= 6 {
            break;
        }
    }

    if snippets.is_empty() {
        println!("No evidence found for query: {}", query);
        return Ok(());
    }

    let prompt = build_evidence_prompt(query, &snippets);
    let max_toks = max_tokens.unwrap_or_else(|| ctx.config.llm.max_tokens);
    let llm = crate::commands::llm::OpenAiClient::from_config(&ctx.config.llm)?;
    let answer = llm.complete(&prompt, max_toks).await?;

    if json {
        let payload = serde_json::json!({
            "answer": answer.trim(),
            "evidence": snippets.iter().map(|(path, start, end, snippet)| serde_json::json!({
                "path": path,
                "start": start,
                "end": end,
                "snippet": snippet,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if matches!(output, OutputLevel::Detailed | OutputLevel::Progress) {
        println!("\nAnswer:\n{}\n", render_markdown_answer(answer.trim()));
    } else {
        println!("{}", answer.trim());
    }
    if matches!(output, OutputLevel::Detailed) {
        println!("Evidence:\n");
        for (i, (path, start, end, snippet)) in snippets.iter().enumerate() {
            println!("[{}] {}:{}-{}", i + 1, path, start, end);
            println!("{}\n", snippet);
        }
    }
    Ok(())
}

fn build_evidence_prompt(query: &str, snippets: &[(String, usize, usize, String)]) -> String {
    let mut evidence = String::new();
    for (i, (path, start, end, snippet)) in snippets.iter().enumerate() {
        evidence.push_str(&format!(
            "[{}] {}:{}-{}\n{}\n\n",
            i + 1,
            path,
            start,
            end,
            snippet
        ));
    }
    format!(
        "You answer questions about this codebase using only the provided code snippets.\n\
Question: {}\n\n\
Evidence:\n{}\n\
Answer the question concisely. Cite file paths and line ranges. If the evidence is insufficient, say so.",
        query, evidence
    )
}

pub fn handle_status(config_path: Option<&Path>) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let ctx = rt.block_on(agent_context::RepoContext::from_env(config_path))?;
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
        if let Ok(db) = sled::open(index_dir.join("store.db")) {
            if let Ok(file_store) = FileStore::new(db) {
                if let Ok(files) = file_store.list_metadata() {
                    println!("Files tracked: {}", files.len());
                }
            }
        }
    }

    // Show recent commit log entries for lineage
    if store_exists {
        let db = sled::open(index_dir.join("store.db"))?;
        let commit_log = CommitLog::new(db)?;
        if let Ok(entries) = commit_log.list(5) {
            if !entries.is_empty() {
                println!("Recent index commits:");
                for entry in entries {
                    println!(" - {} @ {} {}", entry.id, entry.timestamp, entry.note);
                }
            }
        }
    }

    Ok(())
}

#[derive(Default)]
struct IndexStats {
    new_files: usize,
    updated_files: usize,
    removed_files: usize,
    skipped_files: usize,
}
fn build_single_globset(pattern: Option<&str>) -> Option<GlobSet> {
    let pat = pattern?;
    let mut builder = GlobSetBuilder::new();
    if let Ok(glob) = Glob::new(pat) {
        builder.add(glob);
    } else {
        eprintln!("Invalid glob pattern '{}', ignoring.", pat);
        return None;
    }
    match builder.build() {
        Ok(set) => Some(set),
        Err(e) => {
            eprintln!("Failed to build globset: {}", e);
            None
        }
    }
}

fn render_markdown_answer(text: &str) -> String {
    let skin = MadSkin::default();
    let (w, _) = termimad::terminal_size();
    let width = std::cmp::max(20, w.saturating_sub(4) as usize);
    FmtText::from(&skin, text, Some(width)).to_string()
}

fn path_matches(matcher: &Option<GlobSet>, root: &Path, path: &Path) -> bool {
    if let Some(set) = matcher {
        let rel = path.strip_prefix(root).unwrap_or(path);
        set.is_match(rel.to_string_lossy().as_ref())
    } else {
        true
    }
}

fn rank_cfg(config: &Config) -> coderet_core::ranking::RankConfig {
    coderet_core::ranking::RankConfig {
        lexical_weight: config.ranking.lexical,
        vector_weight: config.ranking.vector,
        graph_weight: config.ranking.graph,
        graph_max_depth: config.graph.max_depth,
        graph_decay: config.graph.decay,
        graph_path_weight: config.graph.path_weight,
        symbol_weight: config.ranking.symbol,
        bm25_k1: config.bm25.k1,
        bm25_b: config.bm25.b,
        edge_weights: config.graph.edge_weights.clone(),
        bm25_avg_len: config.bm25.avg_len,
        summary_similarity_threshold: 0.25,
        summary_boost_weight: config.ranking.summary,
    }
}

fn fuse_scores(
    mut hits: Vec<coderet_core::models::ScoredChunk>,
    config: &Config,
    summary_weight: f32,
) -> Vec<coderet_core::models::ScoredChunk> {
    if hits.is_empty() {
        return hits;
    }
    let max_lex = hits
        .iter()
        .filter_map(|h| h.lexical_score)
        .fold(0.0_f32, f32::max);
    let max_vec = hits
        .iter()
        .filter_map(|h| h.vector_score)
        .fold(0.0_f32, f32::max);
    let max_graph = hits
        .iter()
        .filter_map(|h| h.graph_boost)
        .fold(0.0_f32, f32::max);
    let max_sym = hits
        .iter()
        .filter_map(|h| h.symbol_boost)
        .fold(0.0_f32, f32::max);
    let max_sum = hits
        .iter()
        .filter_map(|h| h.summary_score)
        .fold(0.0_f32, f32::max);

    for hit in hits.iter_mut() {
        let lex_n = hit
            .lexical_score
            .map(|v| if max_lex > 0.0 { v / max_lex } else { 0.0 })
            .unwrap_or(0.0);
        let vec_n = hit
            .vector_score
            .map(|v| if max_vec > 0.0 { v / max_vec } else { 0.0 })
            .unwrap_or(0.0);
        let graph_n = hit
            .graph_boost
            .map(|v| if max_graph > 0.0 { v / max_graph } else { 0.0 })
            .unwrap_or(0.0);
        let sym_n = hit
            .symbol_boost
            .map(|v| if max_sym > 0.0 { v / max_sym } else { 0.0 })
            .unwrap_or(0.0);
        let sum_n = hit
            .summary_score
            .map(|v| if max_sum > 0.0 { v / max_sum } else { 0.0 })
            .unwrap_or(0.0);

        hit.score = lex_n * config.ranking.lexical
            + vec_n * config.ranking.vector
            + graph_n * config.ranking.graph
            + sym_n * config.ranking.symbol
            + sum_n * summary_weight;
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits
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
