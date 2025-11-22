use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use core::config::{Config, SearchMode};
use core::chunking::{Chunker, PythonChunker, TypeScriptChunker, JavaChunker, CppChunker};
use core::embeddings::{external::ExternalEmbedder, local::LocalEmbedder};
use core::index::lexical::LexicalIndex;
use core::index::vector::VectorIndex;
use core::retriever::Retriever;
use core::scanner::scan_repo;
use core::structure::index::SymbolIndex;
use core::structure::symbols::SymbolExtractor;
use core::structure::graph::{CodeGraph, GraphBuilder};
use core::summaries::generator::SummaryGenerator;
use core::summaries::index::{Summary, SummaryIndex, SummaryLevel};
use core::ranking::model::{LinearRanker, Ranker};
use std::path::PathBuf;
use std::sync::Arc;

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

pub async fn handle_index(full: bool, summarize: bool) -> Result<()> {
    println!("Indexing repository...");
    let root = std::env::current_dir()?;
    let config = Config::default(); // Load from file if exists

    // 1. Scan
    let scanned_files = scan_repo(&root, &config.index);
    println!("Found {} files to index.", scanned_files.len());

    // 2. Chunk
    let mut all_chunks = Vec::new();
    let py_chunker = PythonChunker::new();
    let ts_chunker = TypeScriptChunker::new();
    let java_chunker = JavaChunker::new();
    let cpp_chunker = CppChunker::new();

    for file in &scanned_files {
        let content = std::fs::read_to_string(&file.path)?;
        let chunks = match file.language {
            core::models::Language::Python => py_chunker.chunk(&content, &file.path)?,
            core::models::Language::TypeScript | core::models::Language::JavaScript => {
                ts_chunker.chunk(&content, &file.path)?
            }
            core::models::Language::Java => java_chunker.chunk(&content, &file.path)?,
            core::models::Language::Cpp => cpp_chunker.chunk(&content, &file.path)?,
            core::models::Language::Unknown => Vec::new(),
        };
        all_chunks.extend(chunks);
    }
    println!("Generated {} chunks.", all_chunks.len());

    // 3. Embed
    println!("Generating embeddings...");
    // Try local embeddings first (fastembed), fall back to external if explicitly requested
    let embedder: Option<Box<dyn core::embeddings::Embedder>> = 
        if std::env::var("USE_OPENAI_EMBEDDINGS").is_ok() {
            // User explicitly wants OpenAI
            match ExternalEmbedder::new(None) {
                Ok(e) => {
                    println!("Using OpenAI embeddings...");
                    Some(Box::new(e))
                },
                Err(e) => {
                    eprintln!("Warning: Failed to initialize OpenAI embeddings: {}", e);
                    None
                }
            }
        } else {
            // Default to local embeddings
            match LocalEmbedder::new(None) {
                Ok(e) => {
                    println!("Using local embeddings (fastembed)...");
                    Some(Box::new(e))
                },
                Err(e) => {
                    eprintln!("Warning: Failed to initialize local embeddings: {}", e);
                    eprintln!("Falling back to OpenAI embeddings...");
                    // Fall back to OpenAI
                    match ExternalEmbedder::new(None) {
                        Ok(ext) => Some(Box::new(ext)),
                        Err(ext_err) => {
                            eprintln!("Warning: Semantic search disabled. {}", ext_err);
                            None
                        }
                    }
                }
            }
        };

    if let Some(embedder) = &embedder {
        let chunk_texts: Vec<String> = all_chunks.iter().map(|c| c.content.clone()).collect();
        let batch_size = 32;
        for (i, batch) in chunk_texts.chunks(batch_size).enumerate() {
            match embedder.embed(batch) {
                Ok(embeddings) => {
                    for (j, emb) in embeddings.into_iter().enumerate() {
                        all_chunks[i * batch_size + j].embedding = Some(emb);
                    }
                }
                Err(e) => eprintln!("Failed to embed batch: {}", e),
            }
        }
    }

    // 4. Index
    let index_dir = root.join(".codeindex");
    std::fs::create_dir_all(&index_dir)?;

    // Symbol Indexing
    println!("Extracting symbols...");
    let mut symbol_index = SymbolIndex::new(&index_dir.join("symbols.json"));
    if full {
        symbol_index.clear();
    }
    
    let mut all_symbols = Vec::new();
    for file in &scanned_files {
        let content = std::fs::read_to_string(&file.path)?;
        if let Ok(symbols) = SymbolExtractor::extract(&content, &file.path, file.language.clone()) {
            all_symbols.extend(symbols);
        }
    }
    symbol_index.add_symbols(all_symbols.clone());
    symbol_index.save()?;
    println!("Indexed {} symbols.", symbol_index.symbols.len());

    // Graph Building
    println!("Building code graph...");
    let mut graph = CodeGraph::new(&index_dir.join("graph.json"));
    if full {
        graph.clear();
    }
    GraphBuilder::build(&mut graph, &all_symbols);
    graph.save()?;
    graph.save()?;
    println!("Graph built with {} nodes and {} edges.", graph.nodes.len(), graph.edges.len());

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
                        core::structure::symbols::SymbolKind::Function | core::structure::symbols::SymbolKind::Method => SummaryLevel::Function,
                        core::structure::symbols::SymbolKind::Class | core::structure::symbols::SymbolKind::Interface => SummaryLevel::Class,
                        _ => continue,
                    };

                    // Check if summary already exists (skip if incremental)
                    if !full && summary_index.get_summary(&symbol.id).is_some() {
                        continue;
                    }

                    // Find content for symbol (naive approach: read file again or use cached chunks?)
                    // We don't have easy access to symbol source text here without reading file.
                    // For Phase 2 prototype, let's read file and extract lines.
                    // This is slow but works.
                    if let Ok(content) = std::fs::read_to_string(&symbol.file_path) {
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

    let lexical_index = LexicalIndex::new(&index_dir.join("lexical"))?;
    // lexical_index.add_chunks(&all_chunks)?; // Needs mut
    // Re-open as mutable or just use it. LexicalIndex::new returns instance.
    // My LexicalIndex implementation has add_chunks taking &mut self.
    // But I need to keep it open?
    // Let's just instantiate and add.
    let mut lexical_index = lexical_index;
    lexical_index.add_chunks(&all_chunks)?;

    let mut vector_index = VectorIndex::new(&index_dir.join("vector.lance")).await?;
    vector_index.add_chunks(&all_chunks).await?;

    println!("Indexing complete.");
    Ok(())
}

pub async fn handle_search(query: String, mode: Option<CliSearchMode>, top: usize, tui: bool, symbol: bool, summary: bool, paths: bool) -> Result<()> {
    if tui {
        println!("TUI not implemented yet.");
        return Ok(());
    }

    let root = std::env::current_dir()?;
    let index_dir = root.join(".codeindex");
    
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
        // Naive search: iterate all summaries and check if ID matches query or text contains query
        // For Phase 2, let's just lookup by ID (exact match) or simple text search
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

    let config = Config::default(); // Load

    let lexical_index = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    let vector_index = Arc::new(VectorIndex::new(&index_dir.join("vector.lance")).await?);
    
    // Try local first, fall back to external
    let embedder: Arc<dyn core::embeddings::Embedder + Send + Sync> = 
        if std::env::var("USE_OPENAI_EMBEDDINGS").is_ok() {
            Arc::new(ExternalEmbedder::new(None)?)
        } else {
            match LocalEmbedder::new(None) {
                Ok(e) => Arc::new(e),
                Err(_) => Arc::new(ExternalEmbedder::new(None)?),
            }
        };

    let mut search_config = config.clone();
    if let Some(m) = mode {
        search_config.search.default_mode = m.into();
    }
    search_config.search.default_top_k = top;

    let core_mode = mode.map_or(config.search.default_mode, |m| m.into());
    let top_k = top;

    // Load Symbol Index for Ranking if available
    let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json")).ok().map(Arc::new);
    
    // Initialize Ranker
    let ranker: Option<Box<dyn Ranker + Send + Sync>> = Some(Box::new(LinearRanker::default()));

    // Load Graph if paths are requested or just generally available
    // For Phase 3, we load it if available to support paths.
    let graph = CodeGraph::load(&index_dir.join("graph.json")).ok().map(Arc::new);

    let retriever = Retriever::new(
        lexical_index,
        vector_index,
        embedder,
        symbol_index,
        graph,
        ranker,
        search_config
    );

    if paths {
        let (chunks, path_results) = retriever.search_paths(&query, core_mode, top_k).await?;
        
        println!("Found {} chunks and {} paths:", chunks.len(), path_results.len());
        
        println!("\n--- Top Chunks ---");
        for (i, (score, chunk)) in chunks.iter().enumerate() {
            println!("\nResult #{}: (Score: {:.4})", i + 1, score);
            println!("File: {}:{}-{}", chunk.file_path.display(), chunk.start_line, chunk.end_line);
            println!("Language: {:?}", chunk.language);
            println!("--------------------------------------------------");
            println!("{}", chunk.content.trim());
            println!("--------------------------------------------------");
        }

        println!("\n--- Top Paths ---");
        for (i, path) in path_results.iter().enumerate() {
            println!("\nPath #{}: (Score: {:.4})", i + 1, path.score);
            for (j, node) in path.nodes.iter().enumerate() {
                let edge_kind = if j > 0 {
                    &path.edges[j-1].kind
                } else {
                    "START"
                };
                println!("  [{}] {} -> {} ({})", edge_kind, node.kind, node.name, node.file_path);
            }
        }

    } else {
        let results = retriever.search(&query, core_mode, top_k).await?;

        println!("Found {} results:", results.len());

        for (i, (score, chunk)) in results.iter().enumerate() {
            println!("\nResult #{}: (Score: {:.4})", i + 1, score);
            println!("File: {}:{}-{}", chunk.file_path.display(), chunk.start_line, chunk.end_line);
            println!("Language: {:?}", chunk.language);
            println!("--------------------------------------------------");
            println!("{}", chunk.content.trim());
            println!("--------------------------------------------------");
        }
    }

    Ok(())
}
