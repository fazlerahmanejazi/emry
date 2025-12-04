use anyhow::{Context, Result};
use emry_config::Config;
use emry_core::chunking::{Chunker, GenericChunker};
use emry_core::models::Language;
use emry_core::relations::{extract_calls_imports, RelationRef};
use emry_core::symbols::extract_symbols;
use emry_core::traits::Embedder;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, warn};
use futures::stream::{self, StreamExt};

/// Input for indexing a single file.
#[derive(Clone)]
pub struct FileInput {
    pub path: PathBuf,
    pub language: Language,
    pub file_id: u64,
    pub file_node_id: String,
    pub hash: String,
    pub content: String,
    pub last_modified: u64,
}

/// Prepared artifacts ready to be written to stores/indices.
#[derive(Clone)]
pub struct PreparedFile {
    pub path: PathBuf,
    pub language: Language,
    pub file_id: u64,
    pub file_node_id: String,
    pub hash: String,
    pub last_modified: u64,
    pub content: String,
    pub chunks: Vec<emry_core::models::Chunk>,
    pub symbols: Vec<emry_core::models::Symbol>,
    pub chunk_symbol_edges: Vec<(String, String)>,
    pub call_edges: Vec<(String, RelationRef)>,
    pub import_edges: Vec<(String, RelationRef)>,
}

pub async fn analyze_source_files(
    inputs: Vec<FileInput>,
    config: &Config,
    concurrency: usize,
) -> Vec<PreparedFile> {
    let cfg = config.clone();
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));

    stream::iter(inputs.into_iter().map(|input| {
        let cfg = cfg.clone();
        let sem = sem.clone();
        let input_clone = input.clone();
        async move {
            let permit = sem.acquire().await.expect("semaphore closed");
            let res = tokio::task::spawn_blocking(move || prepare_file(&input_clone, &cfg))
                .await
                .context("Task join error")
                .and_then(|r| r.context(format!("Failed to prepare file {}", input.path.display())))
                .map_err(|e| {
                    error!("Indexing failed: {:#}", e);
                    e
                })
                .ok();
            drop(permit);
            res
        }
    }))
    .buffer_unordered(concurrency.max(1))
    .filter_map(|r| async move { r })
    .collect()
    .await
}

pub async fn generate_embeddings(
    prepared_files: &mut [PreparedFile],
    embedder: Arc<dyn Embedder + Send + Sync>,
) {
    let mut all_chunks_refs: Vec<&mut emry_core::models::Chunk> = Vec::new();
    for file in prepared_files.iter_mut() {
        for chunk in &mut file.chunks {
            all_chunks_refs.push(chunk);
        }
    }

    if !all_chunks_refs.is_empty() {
        for (i, chunk_batch) in all_chunks_refs.chunks_mut(512).enumerate() {
            let batch_texts: Vec<String> =
                chunk_batch.iter().map(|c| c.content.clone()).collect();
            if let Ok(embeddings) = embedder.embed_batch(&batch_texts).await {
                if embeddings.len() == chunk_batch.len() {
                    for (chunk, emb) in chunk_batch.iter_mut().zip(embeddings) {
                        chunk.embedding = Some(emb);
                    }
                } else {
                    warn!(
                        "Embedding count mismatch in global batch {} (got {}, expected {})",
                        i,
                        embeddings.len(),
                        chunk_batch.len()
                    );
                }
            } else {
                error!("Failed to embed global batch {}", i);
            }
        }
    }
}

pub fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

fn prepare_file(
    input: &FileInput,
    config: &Config,
) -> Result<PreparedFile> {
    let chunker = GenericChunker::with_config(input.language.clone(), config.chunking.clone());
    let mut chunks = chunker.chunk(&input.content, &input.path)?;
    for chunk in chunks.iter_mut() {
        if chunk.content_hash.is_empty() {
            chunk.content_hash = compute_hash(&chunk.content);
        }
    }

    let mut symbols: Vec<emry_core::models::Symbol> = Vec::new();
    let mut chunk_symbol_edges: Vec<(String, String)> = Vec::new();
    match extract_symbols(&input.content, &input.path, &input.language) {
        Ok(syms) => {

            for sym in syms {
                if let Some(chunk_id) = find_covering_chunk_id(&chunks, sym.start_line, sym.end_line) {
                    chunk_symbol_edges.push((chunk_id.clone(), sym.id.clone()));
                } else {
                    for chunk in &chunks {
                        if sym.start_line <= chunk.end_line && sym.end_line >= chunk.start_line {
                            chunk_symbol_edges.push((chunk.id.clone(), sym.id.clone()));
                        }
                    }
                }
                symbols.push(sym);
            }
        }
        Err(e) => {
            warn!("Failed to extract symbols for {}: {}", input.path.display(), e);
        }
    }

    let mut call_edges: Vec<(String, RelationRef)> = Vec::new();

    let (calls, imports) = extract_calls_imports(&input.language, &input.content)?;

    let mut import_edges: Vec<(String, RelationRef)> = Vec::new();

    for c in calls {
        let caller_node = resolve_node_id(c.line, &symbols, &chunks, &input.file_node_id);
        call_edges.push((caller_node, c));
    }
    for imp in imports {
        let caller_node = resolve_node_id(imp.line, &symbols, &chunks, &input.file_node_id);
        import_edges.push((caller_node, imp));
    }

    Ok(PreparedFile {
        path: input.path.clone(),
        language: input.language.clone(),
        file_id: input.file_id,
        file_node_id: input.file_node_id.clone(),
        hash: input.hash.clone(),
        last_modified: input.last_modified,
        content: input.content.clone(),
        chunks,
        symbols,
        chunk_symbol_edges,
        call_edges,
        import_edges,
    })
}

fn find_covering_chunk_id(
    chunks: &[emry_core::models::Chunk],
    start_line: usize,
    end_line: usize,
) -> Option<String> {
    for chunk in chunks {
        if start_line >= chunk.start_line && end_line <= chunk.end_line {
            return Some(chunk.id.clone());
        }
    }
    None
}

fn resolve_node_id(
    line: usize,
    symbols: &[emry_core::models::Symbol],
    chunks: &[emry_core::models::Chunk],
    file_node_id: &str,
) -> String {
    symbols
        .iter()
        .filter(|s| line >= s.start_line && line <= s.end_line)
        .min_by_key(|s| s.end_line - s.start_line)
        .map(|s| s.id.clone())
        .or_else(|| find_covering_chunk_id(chunks, line, line))
        .unwrap_or_else(|| file_node_id.to_string())
}


