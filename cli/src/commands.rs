use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use core::config::{Config, SearchMode};
use core::chunking::{Chunker, PythonChunker, TypeScriptChunker, JavaChunker, CppChunker};
use core::embeddings::{external::ExternalEmbedder, local::LocalEmbedder};
use core::index::lexical::LexicalIndex;
use core::index::vector::VectorIndex;
use core::retriever::Retriever;
use core::scanner::scan_repo;
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

pub async fn handle_index(full: bool) -> Result<()> {
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

    for file in scanned_files {
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

pub async fn handle_search(query: String, mode: Option<CliSearchMode>, top: usize, tui: bool) -> Result<()> {
    if tui {
        println!("TUI not implemented yet.");
        return Ok(());
    }

    let root = std::env::current_dir()?;
    let index_dir = root.join(".codeindex");
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

    let retriever = Retriever::new(lexical_index, vector_index, embedder, search_config);
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

    Ok(())
}
