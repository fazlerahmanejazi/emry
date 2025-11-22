use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use libcore::config::{Config, SearchMode};
use libcore::chunking::{Chunker, PythonChunker, TypeScriptChunker, JavaChunker, CppChunker};
use libcore::index::lexical::LexicalIndex;
use libcore::index::vector::VectorIndex;
use libcore::index::manager::IndexManager;
use libcore::retriever::{Retriever, SearchResult};
use libcore::scanner::scan_repo;
use libcore::structure::index::SymbolIndex;
use libcore::structure::symbols::SymbolExtractor;
use libcore::structure::graph::{CodeGraph, GraphBuilder};
use libcore::summaries::generator::SummaryGenerator;
use libcore::summaries::index::{Summary, SummaryIndex, SummaryLevel};
use libcore::ranking::model::{LinearRanker, Ranker};
use libcore::models::{Language, IndexMetadata};
use globset::{Glob, GlobSetBuilder};
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::Arc;
use std::process::Command;
use reqwest::Client;

mod embeddings_util;

#[derive(Parser)]
#[command(name = "code-retriever")]
#[command(about = "A local code retrieval tool", long_about = None)]
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

        /// Generate summaries (requires OpenAI API key)
        #[arg(long)]
        summarize: bool,
    },
    /// Search the index
    Search {
        /// The query string
        query: String,

        /// Search mode
        #[arg(long, value_enum)]
        mode: Option<CliSearchMode>,

        /// Number of results
        #[arg(long, default_value_t = 10)]
        top: usize,

        /// Filter by language
        #[arg(long)]
        lang: Option<String>,

        /// Filter by path glob
        #[arg(long)]
        path: Option<String>,

        /// Launch TUI
        #[arg(long)]
        tui: bool,

        /// Search for a symbol definition
        #[arg(long)]
        symbol: bool,

        /// Search for a summary
        #[arg(long)]
        summary: bool,

        /// Enable path retrieval
        #[arg(long)]
        paths: bool,

        /// Treat query as regex (lexical/grep-style)
        #[arg(long)]
        regex: bool,

        /// Show scoring breakdown where available
        #[arg(long)]
        explain: bool,

        /// Include summary hits along with code results
        #[arg(long)]
        with_summaries: bool,
    },
    /// Ask a question and get a synthesized answer
    Ask {
        /// The question
        query: String,

        /// Number of results to retrieve
        #[arg(long, default_value_t = 8)]
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

        /// Include summaries in context
        #[arg(long)]
        with_summaries: bool,

        /// Show snippets used for the answer
        #[arg(long)]
        show_snippets: bool,
    },
    /// Show index status
    Status,
    /// Launch the Terminal User Interface
    Tui,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum CliSearchMode {
    Lexical,
    Semantic,
    Hybrid,
}

impl From<CliSearchMode> for SearchMode {
    fn from(mode: CliSearchMode) -> Self {
        match mode {
            CliSearchMode::Lexical => SearchMode::Lexical,
            CliSearchMode::Semantic => SearchMode::Semantic,
            CliSearchMode::Hybrid => SearchMode::Hybrid,
        }
    }
}

pub async fn handle_index(full: bool, summarize: bool, config_path: Option<&Path>) -> Result<()> {
    println!("Indexing repository...");
    let start_time = std::time::Instant::now();
    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);

    // 1. Scan
    let scanned_files = scan_repo(&root, &config.index);
    println!("Found {} files to index.", scanned_files.len());

    // Load previous metadata (for incremental)
    let metadata_path = index_dir.join("meta.json");
    let metadata: IndexMetadata = if full {
        IndexMetadata::default()
    } else {
        IndexManager::load(&metadata_path)
    };

    // Decide which files are new/modified/unchanged
    let mut unchanged: HashMap<PathBuf, (String, Vec<String>)> = HashMap::new();
    let mut to_reindex: Vec<(PathBuf, Language, String)> = Vec::new();
    let mut deleted_chunks: Vec<String> = Vec::new();

    // Mark deletions
    let current_paths: HashSet<PathBuf> = scanned_files.iter().map(|f| f.path.clone()).collect();
    for entry in &metadata.files {
        if !current_paths.contains(&entry.path) {
            deleted_chunks.extend(entry.chunk_ids.clone());
        }
    }

    // Chunkers
    let py_chunker = PythonChunker::new();
    let ts_chunker = TypeScriptChunker::new();
    let java_chunker = JavaChunker::new();
    let cpp_chunker = CppChunker::new();

    let mut new_chunks: Vec<libcore::models::Chunk> = Vec::new();
    let mut file_chunk_map: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut content_cache: HashMap<PathBuf, String> = HashMap::new();

    for file in &scanned_files {
        let content = std::fs::read_to_string(&file.path)?;
        let hash = IndexManager::compute_hash(&content);
        if !full {
            if let Some(prev) = metadata.files.iter().find(|m| m.path == file.path) {
                if prev.content_hash == hash {
                    unchanged.insert(file.path.clone(), (hash, prev.chunk_ids.clone()));
                    content_cache.insert(file.path.clone(), content);
                    continue;
                } else {
                    deleted_chunks.extend(prev.chunk_ids.clone()); // remove old chunks for modified file
                }
            }
        }

        content_cache.insert(file.path.clone(), content.clone());
        to_reindex.push((file.path.clone(), file.language.clone(), content));
    }

    // Chunk only files needing reindex
    let chunk_start = std::time::Instant::now();
    for (path, lang, content) in &to_reindex {
        let chunks = match lang {
            Language::Python => py_chunker.chunk(content, path)?,
            Language::TypeScript | Language::JavaScript => ts_chunker.chunk(content, path)?,
            Language::Java => java_chunker.chunk(content, path)?,
            Language::Cpp => cpp_chunker.chunk(content, path)?,
            Language::Unknown => Vec::new(),
        };
        file_chunk_map.insert(path.clone(), chunks.iter().map(|c| c.id.clone()).collect());
        new_chunks.extend(chunks);
    }
    println!("Re-chunked {} files, {} chunks. ({} ms)", to_reindex.len(), new_chunks.len(), chunk_start.elapsed().as_millis());

    // 3. Embed
    println!("Generating embeddings...");
    let embed_start = std::time::Instant::now();
    let embedder: Option<Arc<dyn libcore::embeddings::Embedder + Send + Sync>> = embeddings_util::select_embedder(&config.embeddings);

    if let Some(embedder) = &embedder {
        let chunk_texts: Vec<String> = new_chunks.iter().map(|c| c.content.clone()).collect();
        let batch_size = 32;
        for (i, batch) in chunk_texts.chunks(batch_size).enumerate() {
            match embedder.embed(batch) {
                Ok(embeddings) => {
                    for (j, emb) in embeddings.into_iter().enumerate() {
                        new_chunks[i * batch_size + j].embedding = Some(emb);
                    }
                }
                Err(e) => eprintln!("Failed to embed batch: {}", e),
            }
        }
    }
    if embedder.is_some() {
        println!("Embeddings generated in {} ms.", embed_start.elapsed().as_millis());
    } else {
        println!("Embeddings skipped.");
    }

    // 4. Index
    if full && index_dir.exists() {
        let _ = std::fs::remove_dir_all(&index_dir);
    }
    std::fs::create_dir_all(&index_dir)?;

    let index_time = std::time::Instant::now();
    // Prepare indexes
    let mut lexical_index = LexicalIndex::new(&index_dir.join("lexical"))?;
    let mut vector_index = VectorIndex::new(&index_dir.join("vector.lance")).await?;

    // Delete stale chunks (modified or deleted files)
    lexical_index.delete_chunks(&deleted_chunks)?;
    vector_index.delete_chunks(&deleted_chunks).await?;

    // Add new/updated chunks
    lexical_index.add_chunks(&new_chunks)?;
    vector_index.add_chunks(&new_chunks).await?;

    // Symbol Indexing (rebuild to handle deletions)
    println!("Extracting symbols...");
    let mut symbol_index = SymbolIndex::new(&index_dir.join("symbols.json"));
    symbol_index.clear();
    let mut all_symbols = Vec::new();
    let mut file_blobs: Vec<(PathBuf, libcore::models::Language, String)> = Vec::new();
    for file in &scanned_files {
        let content = if let Some(cached) = content_cache.get(&file.path) {
            cached.clone()
        } else {
            std::fs::read_to_string(&file.path)?
        };
        if let Ok(symbols) = SymbolExtractor::extract(&content, &file.path, file.language.clone()) {
            all_symbols.extend(symbols);
        }
        file_blobs.push((file.path.clone(), file.language.clone(), content));
    }
    symbol_index.add_symbols(all_symbols.clone());
    symbol_index.save()?;
    println!("Indexed {} symbols.", symbol_index.symbols.len());

    // Graph Building
    println!("Building code graph...");
    let mut graph = CodeGraph::new(&index_dir.join("graph.json"));
    graph.clear();
    GraphBuilder::build(&mut graph, &all_symbols);
    GraphBuilder::build_calls_and_imports(&mut graph, &all_symbols, &file_blobs);
    graph.save()?;
    println!("Graph built with {} nodes and {} edges.", graph.nodes.len(), graph.edges.len());
    println!("Index write took {} ms.", index_time.elapsed().as_millis());

    // Summarization
    if summarize {
        println!("Generating summaries (this may take a while)...");
        let mut summary_index = SummaryIndex::new(&index_dir.join("summaries.json"));
        if full {
            summary_index.clear();
        }

        match SummaryGenerator::new(None) {
            Ok(generator) => {
                let mut count = 0;
                for symbol in &all_symbols {
                    // Only summarize functions and classes for now
                    let level = match symbol.kind {
                        libcore::structure::symbols::SymbolKind::Function | libcore::structure::symbols::SymbolKind::Method => SummaryLevel::Function,
                        libcore::structure::symbols::SymbolKind::Class | libcore::structure::symbols::SymbolKind::Interface => SummaryLevel::Class,
                        _ => continue,
                    };

                    // Check if summary already exists (skip if incremental)
                    if !full && summary_index.get_summary(&symbol.id).is_some() {
                        continue;
                    }

                    let content = content_cache
                        .get(&symbol.file_path)
                        .cloned()
                        .or_else(|| std::fs::read_to_string(&symbol.file_path).ok());
                    if let Some(content) = content {
                        let lines: Vec<&str> = content.lines().collect();
                        if symbol.start_line > 0 && symbol.end_line <= lines.len() {
                            let symbol_text = lines[symbol.start_line-1..symbol.end_line].join("\n");
                            
                            // Context: File path and name
                            let context = format!("File: {}, Symbol: {}", symbol.file_path.display(), symbol.name);
                            
                            match generator.generate(&symbol_text, &context) {
                                Ok(summary_text) => {
                                    let summary = Summary {
                                        id: symbol.id.clone(),
                                        level,
                                        target_id: symbol.id.clone(),
                                        text: summary_text,
                                    };
                                    summary_index.add_summary(summary);
                                    count += 1;
                                    if count % 5 == 0 {
                                        println!("Generated {} summaries...", count);
                                    }
                                }
                                Err(e) => eprintln!("Failed to generate summary for {}: {}", symbol.name, e),
                            }
                        }
                    }
                }
                summary_index.save()?;
                println!("Generated and indexed {} summaries.", count);
            }
            Err(e) => eprintln!("Skipping summarization: {}", e),
        }
    }

    // Build updated metadata
    let mut new_metadata = IndexMetadata {
        version: "1".to_string(),
        files: Vec::new(),
    };
    for file in &scanned_files {
        if let Some((hash, ids)) = unchanged.get(&file.path) {
            IndexManager::update_file_entry(&mut new_metadata, &file.path, hash.clone(), ids.clone());
        } else if let Some(ids) = file_chunk_map.get(&file.path) {
            let hash = IndexManager::compute_hash(
                content_cache
                    .get(&file.path)
                    .map(String::as_str)
                    .unwrap_or(""),
            );
            IndexManager::update_file_entry(&mut new_metadata, &file.path, hash, ids.clone());
        } else if let Some(prev) = metadata.files.iter().find(|m| m.path == file.path) {
            // Fallback: keep previous entry
            IndexManager::update_file_entry(&mut new_metadata, &file.path, prev.content_hash.clone(), prev.chunk_ids.clone());
        }
    }
    IndexManager::save(&metadata_path, &new_metadata)?;

    println!("Indexing complete. New/updated files: {}, unchanged: {}, deleted: {}. Total: {} ms", to_reindex.len(), unchanged.len(), deleted_chunks.len(), start_time.elapsed().as_millis());
    Ok(())
}

pub fn handle_status(config_path: Option<&Path>) -> Result<()> {
    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);

    println!("Repository: {}", root.display());
    println!("Branch: {}", branch);
    println!("Config: default_mode={:?}, top_k={}", config.search.default_mode, config.search.default_top_k);

    if !index_dir.exists() {
        println!("Index: not found at {}", index_dir.display());
        return Ok(());
    }

    let lexical_exists = index_dir.join("lexical").exists();
    let vector_exists = index_dir.join("vector.lance").exists();
    let symbols_path = index_dir.join("symbols.json");
    let summaries_path = index_dir.join("summaries.json");
    let graph_path = index_dir.join("graph.json");

    println!("Index directory: {}", index_dir.display());
    println!(" - Lexical index: {}", if lexical_exists { "present" } else { "missing" });
    println!(" - Vector index: {}", if vector_exists { "present" } else { "missing" });
    println!(" - Symbols: {}", if symbols_path.exists() { "present" } else { "missing" });
    println!(" - Summaries: {}", if summaries_path.exists() { "present" } else { "missing" });
    println!(" - Graph: {}", if graph_path.exists() { "present" } else { "missing" });

    if symbols_path.exists() {
        if let Ok(sym_index) = SymbolIndex::load(&symbols_path) {
            println!("Symbols indexed: {}", sym_index.symbols.len());
        }
    }
    if summaries_path.exists() {
        if let Ok(sum_index) = SummaryIndex::load(&summaries_path) {
            println!("Summaries stored: {}", sum_index.summaries.len());
        }
    }
    if graph_path.exists() {
        if let Ok(graph) = CodeGraph::load(&graph_path) {
            println!("Graph nodes: {}, edges: {}", graph.nodes.len(), graph.edges.len());
        }
    }

    Ok(())
}

pub async fn handle_search(
    query: String,
    mode: Option<CliSearchMode>,
    top: usize,
    lang: Option<String>,
    path: Option<String>,
    tui: bool,
    symbol: bool,
    summary: bool,
    paths: bool,
    regex: bool,
    explain: bool,
    with_summaries: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    eprintln!("DEBUG: handle_search started");
    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();
    
    // Initialize components
    eprintln!("DEBUG: Initializing embedder...");
    let embedder = embeddings_util::select_embedder(&config.embeddings)
        .ok_or_else(|| anyhow::anyhow!("No suitable embedder found. Please configure OpenAI key or Ollama."))?;
    eprintln!("DEBUG: Embedder initialized.");
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);
    // Auto-create index if missing
    eprintln!("DEBUG: Checking index status...");
    if !index_dir.exists() {
        println!("No index found at {}. Creating it now...", index_dir.display());
        handle_index(false, false, config_path).await?;
    } else if config.auto_index_on_search && { eprintln!("DEBUG: Checking needs_reindex..."); needs_reindex(&root, &index_dir, &config)? } {
        println!("Index missing or stale for branch '{}', running incremental index...", branch);
        handle_index(false, false, config_path).await?;
    }
    eprintln!("DEBUG: Index check done.");

    let lang_filter = lang.as_deref().map(Language::from_name);
    let path_matcher = build_single_globset(path.as_deref());

    // Regex/grep mode short-circuit
    if regex {
        let matches = libcore::index::regex::RegexSearcher::search(&root, &query, &config.index)?;
        if matches.is_empty() {
            println!("No matches for regex '{}'.", query);
        } else {
            println!("Regex matches for '{}':", query);
            for (path, line, content) in matches {
                if let Some(ref l) = lang_filter {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if Language::from_extension(ext) != *l {
                            continue;
                        }
                    }
                }
                if !path_matches(&path_matcher, &root, &path) {
                    continue;
                }
                let rel = path.strip_prefix(&root).unwrap_or(&path);
                println!("{}:{}: {}", rel.display(), line, content);
            }
        }
        return Ok(());
    }
    
    if symbol {
        let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json"))?;
        let matches = symbol_index.search(&query);
        println!("Found {} symbol matches:", matches.len());
        for (i, sym) in matches.iter().enumerate() {
            println!("\nMatch #{}: {:?}", i + 1, sym.kind);
            println!("Name: {}", sym.name);
            println!("File: {}:{}-{}", sym.file_path.display(), sym.start_line, sym.end_line);
        }
        return Ok(());
    }

    if summary {
        let summary_path = index_dir.join("summaries.json");
        if !summary_path.exists() {
            println!("No summaries index found. Run 'index --summarize' first.");
            return Ok(());
        }
        let summary_index = SummaryIndex::load(&summary_path)?;
        println!("Searching summaries for '{}'...", query);
        let mut found = false;
        for (id, sum) in &summary_index.summaries {
            if id.contains(&query) || sum.text.contains(&query) {
                println!("\nSummary for {}:", id);
                println!("{}", sum.text);
                found = true;
            }
        }
        if !found {
            println!("No summaries found matching '{}'.", query);
        }
        return Ok(());
    }

    // Determine mode/top before loading indexes
    let mut search_config = config.clone();
    if let Some(m) = mode {
        search_config.search.default_mode = m.into();
    }
    search_config.search.default_top_k = top;

    let core_mode = search_config.search.default_mode.clone();
    let top_k = search_config.search.default_top_k;

    eprintln!("DEBUG: Opening Lexical Index...");
    let lexical_index = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    eprintln!("DEBUG: Lexical Index opened.");

    eprintln!("DEBUG: Opening Vector Index...");
    let vector_index = Arc::new(VectorIndex::new(&index_dir.join("vector.lance")).await?);
    eprintln!("DEBUG: Vector Index opened.");
    
    // Choose embedder only if needed
    let embedder: Option<Arc<dyn libcore::embeddings::Embedder + Send + Sync>> = match core_mode {
        SearchMode::Lexical => None,
        SearchMode::Semantic | SearchMode::Hybrid => {
            embeddings_util::select_embedder(&config.embeddings)
        }
    };

    // Load Symbol Index for Ranking if available
    let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json")).ok().map(Arc::new);
    
    // Initialize Ranker
    let ranker: Option<Box<dyn Ranker + Send + Sync>> = Some(Box::new(LinearRanker::default()));

    // Load Graph if paths are requested or just generally available
    let graph = CodeGraph::load(&index_dir.join("graph.json")).ok().map(Arc::new);

    let embedder_arc: Arc<dyn libcore::embeddings::Embedder + Send + Sync> = match embedder.clone() {
        Some(e) => e,
        None => {
            // Dummy embedder that returns empty vectors (semantic disabled)
            struct NoopEmbedder;
            impl libcore::embeddings::Embedder for NoopEmbedder {
                fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
                    Ok(vec![vec![]; texts.len()])
                }
            }
            Arc::new(NoopEmbedder)
        }
    };

    let effective_mode = match (&embedder, core_mode.clone()) {
        (None, SearchMode::Semantic) | (None, SearchMode::Hybrid) => {
            println!("Semantic/hybrid unavailable (no embedder). Falling back to lexical.");
            SearchMode::Lexical
        }
        _ => core_mode
    };

    let retriever = Retriever::new(
        lexical_index,
        vector_index,
        embedder_arc,
        symbol_index.clone(),
        graph.clone(),
        ranker,
        search_config
    );

    let search_started = std::time::Instant::now();

    if paths {
        eprintln!("DEBUG: Calling search_paths...");
        let (mut chunks, mut path_results) = retriever.search_paths(&query, effective_mode.clone(), top_k).await?;
        eprintln!("DEBUG: search_paths returned.");
        apply_filters(&mut chunks, lang_filter.clone(), &path_matcher, &root);

        if let Some(matcher) = &path_matcher {
            path_results.retain(|p| {
                p.nodes.iter().any(|n| {
                    let pth = PathBuf::from(&n.file_path);
                    matcher.is_match(
                        pth.strip_prefix(&root)
                            .unwrap_or(&pth)
                            .to_string_lossy()
                            .as_ref(),
                    )
                })
            });
        }

        let elapsed = search_started.elapsed();
        println!("Found {} chunks and {} paths ({} ms):", chunks.len(), path_results.len(), elapsed.as_millis());
        
        println!("\n--- Top Chunks ---");
        for (i, res) in chunks.iter().enumerate() {
            println!("\nResult #{}: (Score: {:.4})", i + 1, res.score);
            let chunk = &res.chunk;
            println!("File: {}:{}-{}", chunk.file_path.display(), chunk.start_line, chunk.end_line);
            println!("Language: {}", chunk.language);
            if explain {
                println!(
                    "Lex raw/norm/weight: {:.4} / {:.4} / {:.2}, Sem raw/norm/weight: {:.4} / {:.4} / {:.2}, Final: {:.4}",
                    res.lexical_score_raw,
                    res.lexical_score_norm,
                    res.lexical_weight,
                    res.semantic_score_raw,
                    res.semantic_score_norm,
                    res.semantic_weight,
                    res.score
                );
            }
            println!("--------------------------------------------------");
            print_snippet(chunk, &root, 2);
            println!("--------------------------------------------------");
        }

        println!("\n--- Top Paths ---");
        for (i, path) in path_results.iter().enumerate() {
            println!("\nPath #{}: (Score: {:.4})", i + 1, path.score);
            let len = path.nodes.len();
            for (j, node) in path.nodes.iter().enumerate() {
                let marker = if j == 0 {
                    "[SOURCE]"
                } else if j == len - 1 {
                    "[SINK]  "
                } else {
                    "   |    "
                };
                
                let edge_info = if j > 0 {
                    format!("--[{:?}]-->", path.edges[j-1].kind)
                } else {
                    "          ".to_string()
                };

                if j == 0 {
                    println!(" {} {} ({})", marker, node.name, node.file_path);
                } else {
                    println!(" {} {} {} ({})", marker, edge_info, node.name, node.file_path);
                }
            }
        }

    } else {
        let mut results = retriever.search(&query, effective_mode, top_k).await?;
        apply_filters(&mut results, lang_filter.clone(), &path_matcher, &root);

        let mut summary_matches = Vec::new();
        if with_summaries {
            let summary_path = index_dir.join("summaries.json");
            if summary_path.exists() {
                let summary_index = SummaryIndex::load(&summary_path)?;
                let q_lower = query.to_lowercase();
                for (id, sum) in &summary_index.summaries {
                    if sum.text.to_lowercase().contains(&q_lower) {
                        summary_matches.push((id.clone(), sum.text.clone()));
                    }
                }
            }
        }

        let elapsed = search_started.elapsed();
        println!("Found {} results ({} ms):", results.len(), elapsed.as_millis());

        for (i, res) in results.iter().enumerate() {
            let chunk = &res.chunk;
            println!("\nResult #{}: (Score: {:.4})", i + 1, res.score);
            println!("File: {}:{}-{}", chunk.file_path.display(), chunk.start_line, chunk.end_line);
            println!("Language: {}", chunk.language);
            if explain {
                println!(
                    "Lex raw/norm/weight: {:.4} / {:.4} / {:.2}, Sem raw/norm/weight: {:.4} / {:.4} / {:.2}, Final: {:.4}",
                    res.lexical_score_raw,
                    res.lexical_score_norm,
                    res.lexical_weight,
                    res.semantic_score_raw,
                    res.semantic_score_norm,
                    res.semantic_weight,
                    res.score
                );
            }
            println!("--------------------------------------------------");
            print_snippet(chunk, &root, 2);
            println!("--------------------------------------------------");
        }

        if !summary_matches.is_empty() {
            println!("\n--- Summary matches ---");
            for (id, text) in summary_matches {
                println!("\n{}:\n{}", id, text);
            }
        }
    }

    Ok(())
}

pub async fn handle_ask(
    query: String,
    top: usize,
    mode: Option<CliSearchMode>,
    lang: Option<String>,
    path: Option<String>,
    with_summaries: bool,
    show_snippets: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    eprintln!("DEBUG: handle_search started");
    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);
    if !index_dir.exists() {
        println!("No index found at {}. Creating it now...", index_dir.display());
        handle_index(false, with_summaries, config_path).await?;
    } else if config.auto_index_on_search && needs_reindex(&root, &index_dir, &config)? {
        println!("Index missing or stale for branch '{}', running incremental index...", branch);
        handle_index(false, with_summaries, config_path).await?;
    }

    let lang_filter = lang.as_deref().map(Language::from_name);
    let path_matcher = build_single_globset(path.as_deref());

    let mut search_config = config.clone();
    if let Some(m) = mode {
        search_config.search.default_mode = m.into();
    }
    search_config.search.default_top_k = top;
    let core_mode = search_config.search.default_mode.clone();
    let top_k = search_config.search.default_top_k;

    let lexical_index = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    let vector_index = Arc::new(VectorIndex::new(&index_dir.join("vector.lance")).await?);
    let embedder: Option<Arc<dyn libcore::embeddings::Embedder + Send + Sync>> = match core_mode {
        SearchMode::Lexical => None,
        SearchMode::Semantic | SearchMode::Hybrid => embeddings_util::select_embedder(&config.embeddings),
    };
    let embedder_arc: Arc<dyn libcore::embeddings::Embedder + Send + Sync> = match embedder.clone() {
        Some(e) => e,
        None => {
            struct NoopEmbedder;
            impl libcore::embeddings::Embedder for NoopEmbedder {
                fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
                    Ok(vec![vec![]; texts.len()])
                }
            }
            Arc::new(NoopEmbedder)
        }
    };

    let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json")).ok().map(Arc::new);
    let graph = CodeGraph::load(&index_dir.join("graph.json")).ok().map(Arc::new);
    let ranker: Option<Box<dyn Ranker + Send + Sync>> = Some(Box::new(LinearRanker::default()));

    let retriever = Retriever::new(
        lexical_index,
        vector_index,
        embedder_arc,
        symbol_index,
        graph,
        ranker,
        search_config,
    );

    let mut results = retriever.search(&query, core_mode, top_k).await?;
    apply_filters(&mut results, lang_filter.clone(), &path_matcher, &root);

    let mut summary_matches = Vec::new();
    if with_summaries {
        let summary_path = index_dir.join("summaries.json");
        if summary_path.exists() {
            let summary_index = SummaryIndex::load(&summary_path)?;
            let q_lower = query.to_lowercase();
            for (id, sum) in &summary_index.summaries {
                if sum.text.to_lowercase().contains(&q_lower) {
                    summary_matches.push((id.clone(), sum.text.clone()));
                }
            }
        }
    }

    if results.is_empty() && summary_matches.is_empty() {
        println!("No retrieval results for '{}'.", query);
        return Ok(());
    }

    // Build context
    let mut context_blocks = Vec::new();
    for res in results.iter().take(top_k.min(8)) {
        let chunk = &res.chunk;
        let snippet = collect_snippet(chunk, &root, 2);
        context_blocks.push(format!(
            "[code] {}:{}-{}\n{}",
            chunk.file_path.display(),
            chunk.start_line,
            chunk.end_line,
            snippet
        ));
    }
    for (id, text) in summary_matches.iter().take(4) {
        context_blocks.push(format!("[summary] {}: {}", id, text));
    }

    let prompt = format!(
        "You are a code assistant. Answer concisely based only on the provided context. Cite sources as [file:line].\n\
        Question: {}\n\nContext:\n{}\n\nAnswer with a short paragraph and a Sources list.",
        query,
        context_blocks.join("\n\n")
    );

    if let Some(answer) = call_llm(&prompt).await {
        println!("{}", answer);
    } else {
        println!("LLM unavailable. Showing retrieved snippets:");
        for ctx in context_blocks {
            println!("\n{}\n", ctx);
        }
    }

    if show_snippets {
        println!("\n--- Snippets Used ---");
        for res in results.iter().take(top_k.min(8)) {
            let chunk = &res.chunk;
            println!("\n{}:{}-{}\n{}", chunk.file_path.display(), chunk.start_line, chunk.end_line, collect_snippet(chunk, &root, 2));
        }
    }

    Ok(())
}

fn build_single_globset(pattern: Option<&str>) -> Option<globset::GlobSet> {
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

fn path_matches(matcher: &Option<globset::GlobSet>, root: &Path, path: &Path) -> bool {
    if let Some(set) = matcher {
        let rel = path.strip_prefix(root).unwrap_or(path);
        set.is_match(rel.to_string_lossy().as_ref())
    } else {
        true
    }
}

fn apply_filters(
    results: &mut Vec<SearchResult>,
    lang_filter: Option<Language>,
    path_matcher: &Option<globset::GlobSet>,
    root: &Path,
) {
    results.retain(|res| {
        let chunk = &res.chunk;
        let lang_ok = lang_filter
            .as_ref()
            .map(|l| chunk.language == *l)
            .unwrap_or(true);
        let path_ok = path_matches(path_matcher, root, &chunk.file_path);
        lang_ok && path_ok
    });
}

fn print_snippet(chunk: &libcore::models::Chunk, root: &Path, context: usize) {
    let path = root.join(&chunk.file_path);
    if let Ok(content) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = content.lines().collect();
        let start = chunk.start_line.saturating_sub(1).saturating_sub(context);
        let end = usize::min(lines.len(), chunk.end_line + context);
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_no = start + i + 1;
            let marker = if line_no >= chunk.start_line && line_no <= chunk.end_line { ">" } else { " " };
            println!("{}{:5} {}", marker, line_no, line);
        }
    } else {
        println!("{}", chunk.content.trim());
    }
}

fn collect_snippet(chunk: &libcore::models::Chunk, root: &Path, context: usize) -> String {
    let path = root.join(&chunk.file_path);
    if let Ok(content) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = content.lines().collect();
        let start = chunk.start_line.saturating_sub(1).saturating_sub(context);
        let end = usize::min(lines.len(), chunk.end_line + context);
        let mut out = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_no = start + i + 1;
            let marker = if line_no >= chunk.start_line && line_no <= chunk.end_line { ">" } else { " " };
            out.push_str(&format!("{}{:5} {}\n", marker, line_no, line));
        }
        out
    } else {
        chunk.content.trim().to_string()
    }
}

fn current_branch() -> Option<String> {
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
    {
        if output.status.success() {
            let mut name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if name == "HEAD" {
                // Detached; try short SHA
                if let Ok(out2) = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output() {
                    if out2.status.success() {
                        name = format!("detached_{}", String::from_utf8_lossy(&out2.stdout).trim());
                    }
                }
            }
            return Some(sanitize_branch(&name));
        }
    }
    None
}

fn sanitize_branch(name: &str) -> String {
    name.replace('/', "__").replace(' ', "_")
}

fn needs_reindex(root: &Path, index_dir: &Path, config: &Config) -> Result<bool> {
    let metadata_path = index_dir.join("meta.json");
    if !metadata_path.exists() {
        return Ok(true);
    }
    let metadata = IndexManager::load(&metadata_path);
    let scanned = scan_repo(root, &config.index);
    if scanned.len() != metadata.files.len() {
        return Ok(true);
    }
    let mut meta_map: HashMap<PathBuf, String> = HashMap::new();
    for f in &metadata.files {
        meta_map.insert(f.path.clone(), f.content_hash.clone());
    }
    for file in &scanned {
        if let Some(prev_hash) = meta_map.get(&file.path) {
            let content = fs::read_to_string(&file.path)?;
            let hash = IndexManager::compute_hash(&content);
            if &hash != prev_hash {
                return Ok(true);
            }
        } else {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn call_llm(prompt: &str) -> Option<String> {
    // External if OPENAI_API_KEY, else Ollama-compatible API at LLM_API_BASE or default.
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        let client = Client::new();
        let resp = client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(key)
            .json(&serde_json::json!({
                "model": "gpt-4.1",
                "input": prompt,
                "max_output_tokens": 300
            }))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            eprintln!("LLM call failed: {}", resp.status());
            return None;
        }
        let resp_json = resp.json::<serde_json::Value>().await.ok()?;
        // Parse Responses API shape: output[0].content[0].text
        if let Some(text) = resp_json
            .get("output")
            .and_then(|o| o.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|item| item.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
        {
            return Some(text.trim().to_string());
        }
        // Fallback to chat completion shape if returned
        return resp_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string());
    }

    let base = std::env::var("LLM_API_BASE").unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_string());
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen2.5-coder:1.5b".to_string());
    let client = Client::new();
    let resp = client.post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .json(&serde_json::json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.1,
            "max_tokens": 300
        }))
        .send()
        .await.ok()?
        .json::<serde_json::Value>()
        .await.ok()?;
    resp.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
}
