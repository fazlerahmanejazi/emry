use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSetBuilder};
use libcore::chunking::Chunker;
use libcore::config::{Config, SearchMode};
use libcore::index::lexical::LexicalIndex;
use libcore::index::manager::IndexManager;
use libcore::index::vector::VectorIndex;
use libcore::models::{IndexMetadata, Language};
use libcore::ranking::model::{LinearRanker, Ranker};
use libcore::retriever::{Retriever, SearchResult};
use libcore::scanner::scan_repo;
use libcore::structure::graph::{CodeGraph, GraphBuilder};
use libcore::structure::index::SymbolIndex;
use libcore::structure::symbols::SymbolExtractor;
use libcore::summaries::generator::SummaryGenerator;
use libcore::summaries::index::{Summary, SummaryIndex, SummaryLevel};
use libcore::summaries::vector::SummaryVectorIndex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

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

        /// Override summary levels (comma-separated: function,class,file,module,repo)
        #[arg(long)]
        summarize_levels: Option<String>,

        /// Override summary model
        #[arg(long)]
        summarize_model: Option<String>,

        /// Override summary max tokens
        #[arg(long)]
        summarize_max_tokens: Option<usize>,

        /// Override summary prompt version
        #[arg(long)]
        summarize_prompt_version: Option<String>,
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

        /// Do not apply ignore rules (gitignore/config) for regex/grep search
        #[arg(long, default_value_t = false)]
        no_ignore: bool,

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

        /// Search mode
        #[arg(long, value_enum)]
        mode: Option<CliSearchMode>,

        /// Depth budget for the agent (shallow|default|deep)
        #[arg(long, value_enum, default_value_t = AskDepth::Default)]
        depth: AskDepth,

        /// Include summaries in context
        #[arg(long)]
        with_summaries: bool,
        
        /// Allow agent to use code graph (paths/references) if available
        #[arg(long, default_value_t = true)]
        use_graph: bool,

        /// Allow agent to use symbol index if available
        #[arg(long, default_value_t = true)]
        use_symbols: bool,
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

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum AskDepth {
    Shallow,
    Default,
    Deep,
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

impl From<AskDepth> for libcore::agent::brain::AgentDepth {
    fn from(d: AskDepth) -> Self {
        match d {
            AskDepth::Shallow => libcore::agent::brain::AgentDepth::Shallow,
            AskDepth::Default => libcore::agent::brain::AgentDepth::Default,
            AskDepth::Deep => libcore::agent::brain::AgentDepth::Deep,
        }
    }
}

fn symbol_level(kind: &libcore::structure::symbols::SymbolKind) -> Option<SummaryLevel> {
    match kind {
        libcore::structure::symbols::SymbolKind::Function
        | libcore::structure::symbols::SymbolKind::Method => Some(SummaryLevel::Function),
        libcore::structure::symbols::SymbolKind::Class
        | libcore::structure::symbols::SymbolKind::Interface => Some(SummaryLevel::Class),
        _ => None,
    }
}

async fn auto_summarize_if_needed(
    index_dir: &Path,
    config: &Config,
) -> Result<()> {
    if !config.summaries.enabled || !config.summaries.auto_on_query {
        return Ok(());
    }
    let symbols_path = index_dir.join("symbols.json");
    if !symbols_path.exists() {
        return Ok(());
    }

    let symbol_index = SymbolIndex::load(&symbols_path)?;
    let mut summary_index = SummaryIndex::new(&index_dir.join("summaries.json"));

    let mut content_cache: HashMap<PathBuf, String> = HashMap::new();
    let mut candidates = Vec::new();

    for sym_list in symbol_index.symbols.values() {
        for sym in sym_list {
            let level = match symbol_level(&sym.kind) {
                Some(lv) => lv,
                None => continue,
            };
            if !config.summaries.levels.contains(&level) {
                continue;
            }

            let content = content_cache
                .entry(sym.file_path.clone())
                .or_insert_with(|| std::fs::read_to_string(&sym.file_path).unwrap_or_default())
                .clone();
            if content.is_empty() {
                continue;
            }
            let lines: Vec<&str> = content.lines().collect();
            if sym.start_line == 0 || sym.end_line == 0 || sym.end_line > lines.len() {
                continue;
            }
            let snippet = lines[sym.start_line - 1..sym.end_line].join("\n");
            if snippet.trim().is_empty() {
                continue;
            }

            let mut hasher = Sha256::new();
            hasher.update(snippet.as_bytes());
            let source_hash = hex::encode(hasher.finalize());

            if let Some(existing) = summary_index.get_summary(&sym.id) {
                if existing
                    .source_hash
                    .as_ref()
                    .map(|h| h == &source_hash)
                    .unwrap_or(false)
                    && existing
                        .prompt_version
                        .as_ref()
                        .map(|v| v == &config.summaries.prompt_version)
                        .unwrap_or(false)
                {
                    continue;
                }
            }

            candidates.push((sym.clone(), snippet, level, source_hash));
        }
    }

    if candidates.is_empty() {
        return Ok(());
    }

    let total = candidates.len();
    let avg_input_tokens: usize = candidates
        .iter()
        .map(|(_, snip, _, _)| snip.len() / 4 + 100)
        .sum::<usize>()
        / total.max(1);
    let est_tokens = total * (avg_input_tokens + config.summaries.max_tokens);
    println!(
        "Auto-summarizing {} symbols (est. {} tokens) with model {}...",
        total, est_tokens, config.summaries.model
    );

    let generator = match SummaryGenerator::new(
        Some(config.summaries.model.clone()),
        config.summaries.max_tokens,
        config.summaries.prompt_version.clone(),
        config.summaries.retries,
    ) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Skipping auto-summarize (no LLM available): {}", e);
            return Ok(());
        }
    };

    let embedder = embeddings_util::select_embedder(&config.embeddings)
        .ok_or_else(|| anyhow::anyhow!("No suitable embedder found for summaries embed"))?;

    let mut generated_summaries = Vec::new();
    for (sym, snippet, level, source_hash) in candidates {
        let context = format!("File: {}, Symbol: {}", sym.file_path.display(), sym.name);
        let generated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let level_val = level.clone();
        let source_hash_val = source_hash.clone();

        let summary_text = match generator.generate(&snippet, &context) {
            Ok(text) if !text.trim().is_empty() => text,
            _ => snippet
                .lines()
                .find(|l| !l.trim().is_empty())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| sym.name.clone()),
        };

        let embedding = embedder
            .embed(&[summary_text.clone()])
            .ok()
            .and_then(|mut v| v.pop());

        let summary = Summary {
            id: sym.id.clone(),
            level: level_val,
            target_id: sym.id.clone(),
            text: summary_text,
            file_path: Some(sym.file_path.clone()),
            start_line: Some(sym.start_line),
            end_line: Some(sym.end_line),
            name: Some(sym.name.clone()),
            language: Some(sym.language.to_string()),
            model: Some(config.summaries.model.clone()),
            prompt_version: Some(config.summaries.prompt_version.clone()),
            generated_at: Some(generated_at),
            source_hash: Some(source_hash_val),
            embedding,
        };
        summary_index.add_summary(summary.clone());
        generated_summaries.push(summary);
    }

    summary_index.save()?;
    if !generated_summaries.is_empty() {
        if let Ok(mut vec_idx) = SummaryVectorIndex::new(&index_dir.join("summary.lance")).await {
            let _ = vec_idx.add_summaries(&generated_summaries, false).await;
        }
    }
    Ok(())
}

pub async fn handle_index(
    full: bool,
    summarize: bool,
    summarize_levels: Option<String>,
    summarize_model: Option<String>,
    summarize_max_tokens: Option<usize>,
    summarize_prompt_version: Option<String>,
    config_path: Option<&Path>,
) -> Result<()> {
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
    let mut total_lines: usize = 0; // Track lines as we read files

    // Mark deletions
    let current_paths: HashSet<PathBuf> = scanned_files.iter().map(|f| f.path.clone()).collect();
    for entry in &metadata.files {
        if !current_paths.contains(&entry.path) {
            deleted_chunks.extend(entry.chunk_ids.clone());
        }
    }

    // Chunkers
    // Use GenericChunker for all languages
    use libcore::chunking::GenericChunker;

    let mut new_chunks: Vec<libcore::models::Chunk> = Vec::new();
    let mut file_chunk_map: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut content_cache: HashMap<PathBuf, String> = HashMap::new();

    for file in &scanned_files {
        let content = std::fs::read_to_string(&file.path)?;
        let hash = IndexManager::compute_hash(&content);
        
        // Count lines efficiently (while we already have content in memory)
        total_lines += content.lines().count();
        
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
        let chunker = GenericChunker::with_config(lang.clone(), config.index.chunking.clone());
        let chunks = chunker.chunk(content, path)?;
        file_chunk_map.insert(path.clone(), chunks.iter().map(|c| c.id.clone()).collect());
        new_chunks.extend(chunks);
    }
    println!(
        "Processed {} files ({} lines), {} chunks. ({} ms)",
        to_reindex.len(),
        total_lines,
        new_chunks.len(),
        chunk_start.elapsed().as_millis()
    );

    // 3. Embed
    println!("Generating embeddings...");
    let embed_start = std::time::Instant::now();
    let embedder: Option<Arc<dyn libcore::embeddings::Embedder + Send + Sync>> =
        embeddings_util::select_embedder(&config.embeddings);

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
        println!(
            "Embeddings generated in {} ms.",
            embed_start.elapsed().as_millis()
        );
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
    let mut chunks_by_file: HashMap<PathBuf, Vec<libcore::models::Chunk>> = HashMap::new();
    for chunk in &new_chunks {
        chunks_by_file
            .entry(chunk.file_path.clone())
            .or_default()
            .push(chunk.clone());
    }
    for file in &scanned_files {
        let content = if let Some(cached) = content_cache.get(&file.path) {
            cached.clone()
        } else {
            std::fs::read_to_string(&file.path)?
        };
        if let Ok(mut symbols) =
            SymbolExtractor::extract(&content, &file.path, file.language.clone())
        {
            if let Some(chunks) = chunks_by_file.get(&file.path) {
                for sym in symbols.iter_mut() {
                    let mut covering = Vec::new();
                    for ch in chunks {
                        if ranges_overlap(sym.start_line, sym.end_line, ch.start_line, ch.end_line)
                        {
                            covering.push(ch.id.clone());
                        }
                    }
                    if !covering.is_empty() {
                        sym.chunk_ids = covering.clone();
                        sym.chunk_id = covering.first().cloned();
                    }
                }
            }
            all_symbols.extend(symbols);
        }
        file_blobs.push((file.path.clone(), file.language.clone(), content));
    }
    symbol_index.add_symbols(all_symbols.clone());
    symbol_index.save()?;
    println!("Indexed {} symbols.", symbol_index.symbols.len());

    // Graph Building (incremental where possible)
    println!("Building code graph...");
    let mut graph = if full {
        let mut g = CodeGraph::new(&index_dir.join("graph.json"));
        g.clear();
        g
    } else {
        CodeGraph::new(&index_dir.join("graph.json"))
    };
    if !full {
        let mut changed: HashSet<PathBuf> = to_reindex.iter().map(|(p, _, _)| p.clone()).collect();
        for entry in &metadata.files {
            if !current_paths.contains(&entry.path) {
                changed.insert(entry.path.clone());
            }
        }
        if !changed.is_empty() {
            GraphBuilder::prune_files(&mut graph, &changed);
        }
        let changed_symbols: Vec<_> = all_symbols
            .iter()
            .filter(|s| changed.contains(&s.file_path))
            .cloned()
            .collect();
        let changed_blobs: Vec<_> = file_blobs
            .iter()
            .filter(|(p, _, _)| changed.contains(p))
            .cloned()
            .collect();
        GraphBuilder::build(&mut graph, &changed_symbols);
        GraphBuilder::build_calls_and_imports(&mut graph, &changed_symbols, &changed_blobs);
    } else {
        GraphBuilder::build(&mut graph, &all_symbols);
        GraphBuilder::build_calls_and_imports(&mut graph, &all_symbols, &file_blobs);
    }
    graph.save()?;
    println!(
        "Graph built with {} nodes and {} edges.",
        graph.nodes.len(),
        graph.edges.len()
    );
    println!("Index write took {} ms.", index_time.elapsed().as_millis());

    // Summarization
    if summarize && config.summaries.enabled {
        let mut summary_conf = config.summaries.clone();
        if let Some(m) = summarize_model {
            summary_conf.model = m;
        }
        if let Some(toks) = summarize_max_tokens {
            summary_conf.max_tokens = toks;
        }
        if let Some(pv) = summarize_prompt_version {
            summary_conf.prompt_version = pv;
        }
        if let Some(levels_str) = summarize_levels {
            let mut levels = Vec::new();
            for part in levels_str.split(',') {
                match part.trim().to_lowercase().as_str() {
                    "function" => levels.push(SummaryLevel::Function),
                    "class" => levels.push(SummaryLevel::Class),
                    "file" => levels.push(SummaryLevel::File),
                    "module" => levels.push(SummaryLevel::Module),
                    "repo" => levels.push(SummaryLevel::Repo),
                    _ => {}
                }
            }
            if !levels.is_empty() {
                summary_conf.levels = levels;
            }
        }
        println!("Generating summaries (this may take a while)...");
        let mut summary_index = SummaryIndex::new(&index_dir.join("summaries.json"));
        if full {
            summary_index.clear();
        }

        let embedder_for_summaries =
            embeddings_util::select_embedder(&config.embeddings);

        match SummaryGenerator::new(
            Some(summary_conf.model.clone()),
            summary_conf.max_tokens,
            summary_conf.prompt_version.clone(),
            summary_conf.retries,
        ) {
            Ok(generator) => {
        let mut count = 0;
                let mut generated_summaries = Vec::new();
                for symbol in &all_symbols {
                    // Only summarize functions and classes for now
                    let level = match symbol.kind {
                        libcore::structure::symbols::SymbolKind::Function
                        | libcore::structure::symbols::SymbolKind::Method => SummaryLevel::Function,
                        libcore::structure::symbols::SymbolKind::Class
                        | libcore::structure::symbols::SymbolKind::Interface => SummaryLevel::Class,
                        _ => continue,
                    };
                    if !summary_conf.levels.contains(&level) {
                        continue;
                    }

                    // Check if summary already exists (skip if incremental)
                    if !full {
                        if let Some(existing) = summary_index.get_summary(&symbol.id) {
                            // Skip if source hash and prompt version match
                            if existing
                                .prompt_version
                                .as_ref()
                                .map(|v| v == &summary_conf.prompt_version)
                                .unwrap_or(false)
                            {
                                // We will compute hash below; defer decision
                            } else {
                                // continue to regenerate
                            }
                        }
                    }

                    let content = content_cache
                        .get(&symbol.file_path)
                        .cloned()
                        .or_else(|| std::fs::read_to_string(&symbol.file_path).ok());
                    if let Some(content) = content {
                        let lines: Vec<&str> = content.lines().collect();
                        if symbol.start_line > 0 && symbol.end_line <= lines.len() {
                            let symbol_text =
                                lines[symbol.start_line - 1..symbol.end_line].join("\n");
                            let mut hasher = Sha256::new();
                            hasher.update(symbol_text.as_bytes());
                            let source_hash = hex::encode(hasher.finalize());

                            if !full {
                                if let Some(existing) = summary_index.get_summary(&symbol.id) {
                                    if existing
                                        .source_hash
                                        .as_ref()
                                        .map(|h| h == &source_hash)
                                        .unwrap_or(false)
                                        && existing
                                            .prompt_version
                                            .as_ref()
                                            .map(|v| v == &summary_conf.prompt_version)
                                            .unwrap_or(false)
                                    {
                                        continue;
                                    }
                                }
                            }

                            // Context: File path and name
                            let context = format!(
                                "File: {}, Symbol: {}",
                                symbol.file_path.display(),
                                symbol.name
                            );

                            let generated_at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);

                            let summary_text = match generator.generate(&symbol_text, &context) {
                                Ok(text) => text,
                                Err(e) => {
                                    eprintln!(
                                        "Failed to generate summary for {}: {}, using fallback",
                                        symbol.name, e
                                    );
                                    // Fallback: first meaningful line or trimmed slice
                                    symbol_text
                                        .lines()
                                        .find(|l| !l.trim().is_empty())
                                        .map(|s| s.trim().to_string())
                                        .unwrap_or_else(|| symbol.name.clone())
                                }
                            };

                            let embedding = embedder_for_summaries
                                .as_ref()
                                .and_then(|emb| emb.embed(&[summary_text.clone()]).ok())
                                .and_then(|mut v| v.pop());

                            let summary = Summary {
                                id: symbol.id.clone(),
                                level,
                                target_id: symbol.id.clone(),
                                text: summary_text.clone(),
                                file_path: Some(symbol.file_path.clone()),
                                start_line: Some(symbol.start_line),
                                end_line: Some(symbol.end_line),
                                name: Some(symbol.name.clone()),
                                language: Some(symbol.language.to_string()),
                                model: Some(summary_conf.model.clone()),
                                prompt_version: Some(summary_conf.prompt_version.clone()),
                                generated_at: Some(generated_at),
                                source_hash: Some(source_hash),
                                embedding,
                            };
                            summary_index.add_summary(summary.clone());
                            generated_summaries.push(summary);
                            count += 1;
                            if count % 5 == 0 {
                                println!("Generated {} summaries...", count);
                            }
                        }
                    }
                }
                summary_index.save()?;
                if !generated_summaries.is_empty() {
                    let mut vec_idx =
                        SummaryVectorIndex::new(&index_dir.join("summary.lance")).await?;
                    vec_idx.add_summaries(&generated_summaries, full).await?;
                }
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
            IndexManager::update_file_entry(
                &mut new_metadata,
                &file.path,
                hash.clone(),
                ids.clone(),
            );
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
            IndexManager::update_file_entry(
                &mut new_metadata,
                &file.path,
                prev.content_hash.clone(),
                prev.chunk_ids.clone(),
            );
        }
    }
    IndexManager::save(&metadata_path, &new_metadata)?;

    println!(
        "Indexing complete. New/updated files: {}, unchanged: {}, deleted: {}. Total: {} ms",
        to_reindex.len(),
        unchanged.len(),
        deleted_chunks.len(),
        start_time.elapsed().as_millis()
    );
    Ok(())
}

pub fn handle_status(config_path: Option<&Path>) -> Result<()> {
    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);

    println!("Repository: {}", root.display());
    println!("Branch: {}", branch);
    println!(
        "Config: default_mode={:?}, top_k={}",
        config.search.default_mode, config.search.default_top_k
    );

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
    println!(
        " - Lexical index: {}",
        if lexical_exists { "present" } else { "missing" }
    );
    println!(
        " - Vector index: {}",
        if vector_exists { "present" } else { "missing" }
    );
    println!(
        " - Symbols: {}",
        if symbols_path.exists() {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        " - Summaries: {}",
        if summaries_path.exists() {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        " - Graph: {}",
        if graph_path.exists() {
            "present"
        } else {
            "missing"
        }
    );

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
            println!(
                "Graph nodes: {}, edges: {}",
                graph.nodes.len(),
                graph.edges.len()
            );
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
    _tui: bool,
    symbol: bool,
    summary: bool,
    paths: bool,
    regex: bool,
    no_ignore: bool,
    explain: bool,
    with_summaries: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    eprintln!("DEBUG: handle_search started");
    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();

    // Initialize components (embedder deferred until needed)
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);
    // Auto-create index if missing
    eprintln!("DEBUG: Checking index status...");
    if !index_dir.exists() {
        println!(
            "No index found at {}. Creating it now...",
            index_dir.display()
        );
        handle_index(false, false, None, None, None, None, config_path).await?;
    } else if config.auto_index_on_search && {
        eprintln!("DEBUG: Checking needs_reindex...");
        needs_reindex(&root, &index_dir, &config)?
    } {
        println!(
            "Index missing or stale for branch '{}', running incremental index...",
            branch
        );
        handle_index(false, false, None, None, None, None, config_path).await?;
    }
    eprintln!("DEBUG: Index check done.");

    let lang_filter = lang.as_deref().map(Language::from_name);
    let path_matcher = build_single_globset(path.as_deref());

    // Regex/grep mode short-circuit
    if regex {
        let matches = libcore::index::regex::RegexSearcher::search_with_ignore(
            &root,
            &query,
            &config.index,
            !no_ignore,
        )?;
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
            println!(
                "File: {}:{}-{}",
                sym.file_path.display(),
                sym.start_line,
                sym.end_line
            );
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
        let embedder = embeddings_util::select_embedder(&config.embeddings).ok_or_else(|| {
            anyhow::anyhow!("No suitable embedder found for summary search. Configure OpenAI/Ollama.")
        })?;
        println!("Semantic summary search for '{}'...", query);
        let results = summary_index.semantic_search(&query, embedder.as_ref(), top)?;
        if results.is_empty() {
            println!("No summaries found matching '{}'.", query);
        } else {
            for (i, (score, sum)) in results.iter().enumerate() {
                let loc = sum
                    .file_path
                    .as_ref()
                    .map(|p| format!("{}:{}-{}", p.display(), sum.start_line.unwrap_or(0), sum.end_line.unwrap_or(0)))
                    .unwrap_or_else(|| sum.target_id.clone());
                println!("\nSummary #{} (score {:.3})", i + 1, score);
                println!("Target: {}", loc);
                println!("{}", sum.text);
            }
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
    let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json"))
        .ok()
        .map(Arc::new);
    let summary_index = SummaryIndex::load(&index_dir.join("summaries.json"))
        .ok()
        .map(Arc::new);
    let mut summary_vector = SummaryVectorIndex::new(&index_dir.join("summary.lance")).await.ok();

    // Initialize Ranker
    let ranker: Option<Box<dyn Ranker + Send + Sync>> = Some(Box::new(LinearRanker::default()));

    // Load Graph if paths are requested or just generally available
    let graph = CodeGraph::load(&index_dir.join("graph.json"))
        .ok()
        .map(Arc::new);

    let embedder_arc: Arc<dyn libcore::embeddings::Embedder + Send + Sync> = match embedder.clone()
    {
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
        _ => core_mode,
    };

    let retriever = Retriever::new(
        lexical_index,
        vector_index,
        embedder_arc,
        symbol_index.clone(),
        summary_index.clone(),
        graph.clone(),
        ranker,
        summary_vector.take(),
    );

    let search_started = std::time::Instant::now();

    if paths {
        eprintln!("DEBUG: Calling search_paths...");
        let (mut chunks, mut path_results) = retriever
            .search_paths(&query, effective_mode.clone(), top_k)
            .await?;
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
        println!(
            "Found {} chunks and {} paths ({} ms):",
            chunks.len(),
            path_results.len(),
            elapsed.as_millis()
        );

        println!("\n--- Top Chunks ---");
        for (i, res) in chunks.iter().enumerate() {
            println!("\nResult #{}: (Score: {:.4})", i + 1, res.score);
            let chunk = &res.chunk;
            println!(
                "File: {}:{}-{}",
                chunk.file_path.display(),
                chunk.start_line,
                chunk.end_line
            );
            println!("Language: {}", chunk.language);
            if !chunk.scope_path.is_empty() {
                println!("Scope: {}", chunk.scope_path.join(" -> "));
            } else if let Some(scope) = &chunk.parent_scope {
                println!("Scope: {}", scope);
            }
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
                    format!("--[{:?}]-->", path.edges[j - 1].kind)
                } else {
                    "          ".to_string()
                };

                if j == 0 {
                    println!(" {} {} ({})", marker, node.name, node.file_path);
                } else {
                    println!(
                        " {} {} {} ({})",
                        marker, edge_info, node.name, node.file_path
                    );
                }
            }
        }
    } else {
        let (mut results, summary_hits) = if with_summaries {
            retriever
                .search_with_summaries(
                    &query,
                    effective_mode,
                    top_k,
                    config.search.summary_boost_weight,
                    config.search.summary_similarity_threshold,
                )
                .await?
        } else {
            (retriever.search(&query, effective_mode, top_k).await?, Vec::new())
        };
        apply_filters(&mut results, lang_filter.clone(), &path_matcher, &root);

        let elapsed = search_started.elapsed();
        println!(
            "Found {} results ({} ms):",
            results.len(),
            elapsed.as_millis()
        );

        for (i, res) in results.iter().enumerate() {
            let chunk = &res.chunk;
            println!("\nResult #{}: (Score: {:.4})", i + 1, res.score);
            println!(
                "File: {}:{}-{}",
                chunk.file_path.display(),
                chunk.start_line,
                chunk.end_line
            );
            println!("Language: {}", chunk.language);
            if let Some(dist) = res.graph_distance {
                println!("Graph: distance={} boost={:.2}", dist, res.graph_boost);
            }
            if !chunk.scope_path.is_empty() {
                println!("Scope: {}", chunk.scope_path.join(" -> "));
            } else if let Some(scope) = &chunk.parent_scope {
                println!("Scope: {}", scope);
            }
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

        // Show a lightweight best path for top results if graph is available
        if let Some(graph) = graph.as_ref() {
            println!("\n--- Graph paths (top 3 results) ---");
            let builder = libcore::paths::builder::PathBuilder::new(graph);
            let mut cfg = libcore::paths::builder::PathBuilderConfig::default();
            cfg.max_length = 4;
            cfg.max_paths = 1;
            for res in results.iter().take(3) {
                if let Some(node_id) = graph.find_node_for_chunk(&res.chunk) {
                    let paths = builder.find_paths(&node_id, &cfg);
                    if let Some(path) = paths.first() {
                        println!(
                            "\n{}:{}-{}",
                            res.chunk.file_path.display(),
                            res.chunk.start_line,
                            res.chunk.end_line
                        );
                        for (idx, node) in path.nodes.iter().enumerate() {
                            if idx > 0 {
                                let edge = &path.edges[idx - 1];
                                println!("  -- {:?} --> {}", edge.kind, node.name);
                            } else {
                                println!("  {}", node.name);
                            }
                        }
                    }
                }
            }
        }

        if !summary_hits.is_empty() {
            println!("\n--- Summary matches ---");
            for sum in summary_hits {
                let loc = sum
                    .file_path
                    .as_ref()
                    .map(|p| format!("{}:{}-{}", p.display(), sum.start_line.unwrap_or(0), sum.end_line.unwrap_or(0)))
                    .unwrap_or_else(|| sum.target_id.clone());
                println!("\n{} | {}\n{}", sum.id, loc, sum.text);
            }
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
            let marker = if line_no >= chunk.start_line && line_no <= chunk.end_line {
                ">"
            } else {
                " "
            };
            println!("{}{:5} {}", marker, line_no, line);
        }
    } else {
        println!("{}", chunk.content.trim());
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
                if let Ok(out2) = Command::new("git")
                    .args(["rev-parse", "--short", "HEAD"])
                    .output()
                {
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

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    let start = std::cmp::max(a_start, b_start);
    let end = std::cmp::min(a_end, b_end);
    start <= end
}



pub async fn handle_ask(
    query: String,
    mode: Option<CliSearchMode>,
    depth: AskDepth,
    with_summaries: bool,
    use_graph: bool,
    use_symbols: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    use libcore::agent::brain::Agent;
    use libcore::agent::brain::AgentOptions;
    use libcore::agent::llm::LLMClient;
    use libcore::agent::tools::{ToolRegistry, SearchTool, PathTool, ReadCodeTool, GrepTool, SymbolTool, ReferencesTool, ListDirTool, SummaryTool};

    let root = std::env::current_dir()?;
    let config = Config::load_from(config_path).unwrap_or_default();
    let branch = current_branch().unwrap_or_else(|| "default".to_string());
    let index_dir = root.join(".codeindex").join("branches").join(&branch);

    // Auto-create index if missing (like handle_search does)
    if !index_dir.exists() {
        println!(
            "No index found at {}. Creating it now...",
            index_dir.display()
        );
        handle_index(false, false, None, None, None, None, config_path).await?;
    } else if config.auto_index_on_search && needs_reindex(&root, &index_dir, &config)? {
        println!(
            "Index missing or stale for branch '{}', running incremental index...",
            branch
        );
        handle_index(false, false, None, None, None, None, config_path).await?;
    }

    // Optional auto-summarize before agent (incremental: missing/stale only)
    if config.summaries.auto_on_query && config.summaries.enabled {
        auto_summarize_if_needed(&index_dir, &config).await?;
    }

    // Initialize components
    let lexical_index = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    let vector_index = Arc::new(VectorIndex::new(&index_dir.join("vector.lance")).await?);
    
    let embedder = embeddings_util::select_embedder(&config.embeddings).ok_or_else(|| {
        anyhow::anyhow!("No suitable embedder found.")
    })?;

    let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json")).ok().map(Arc::new);
    let graph = CodeGraph::load(&index_dir.join("graph.json")).ok().map(Arc::new);
    let ranker: Option<Box<dyn Ranker + Send + Sync>> = Some(Box::new(LinearRanker::default()));
    let summaries_path = index_dir.join("summaries.json");
    let summary_index = SummaryIndex::load(&summaries_path).ok().map(Arc::new);
    let mut summary_vector = SummaryVectorIndex::new(&index_dir.join("summary.lance")).await.ok();

    let retriever = Arc::new(Retriever::new(
        lexical_index,
        vector_index,
        embedder,
        symbol_index.clone(),
        summary_index.clone(),
        graph.clone(),
        ranker,
        summary_vector.take(),
    ));

    // Setup Tools
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SearchTool::new(retriever)));
    registry.register(Arc::new(ReadCodeTool::new()));
    registry.register(Arc::new(GrepTool::new()));
    registry.register(Arc::new(ReferencesTool::new()));
    registry.register(Arc::new(ListDirTool::new()));
    if let Some(sum) = summary_index.clone() {
        registry.register(Arc::new(SummaryTool::new(sum)));
    }
    
    if use_symbols {
        if let Some(idx) = symbol_index {
            registry.register(Arc::new(SymbolTool::new(idx)));
        }
    }
    
    if use_graph {
        if let Some(g) = graph {
            registry.register(Arc::new(PathTool::new(g)));
        }
    }

    // Setup Agent
    let llm = LLMClient::new(None)?;
    let agent = Agent::new(llm, registry);
    let agent_opts = AgentOptions {
        top: Some(config.search.default_top_k),
        mode: mode.map(|m| m.into()).or(Some(config.search.default_mode)),
        lang: None,
        path: None,
        with_summaries,
        depth: depth.into(),
    };

    println!("Agent is thinking...");
    let answer = agent.ask(&query, agent_opts).await?;
    println!("\nAgent Answer:\n{}", answer);

    Ok(())
}
