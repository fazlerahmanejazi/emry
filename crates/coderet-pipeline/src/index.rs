use anyhow::Result;
use coderet_config::Config;
use coderet_core::chunking::Chunker;
use coderet_core::models::Language;
use coderet_core::relations::{extract_calls_imports, RelationRef};
use coderet_core::symbols::extract_symbols;
use coderet_core::traits::Embedder;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, warn};

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
pub struct PreparedFile {
    pub path: PathBuf,
    pub file_id: u64,
    pub file_node_id: String,
    pub hash: String,
    pub last_modified: u64,
    pub content: String,
    pub chunks: Vec<coderet_core::models::Chunk>,
    pub symbols: Vec<coderet_core::models::Symbol>,
    pub chunk_symbol_edges: Vec<(String, String)>, // chunk -> symbol
    pub call_edges: Vec<(String, String)>,         // caller -> callee name
    pub import_edges: Vec<(String, String)>,       // file or chunk -> import name
}

pub async fn prepare_files_async(
    inputs: Vec<FileInput>,
    config: &Config,
    embedder: Option<Arc<dyn Embedder + Send + Sync>>,
    concurrency: usize,
) -> Vec<PreparedFile> {
    use futures::stream::{self, StreamExt};
    let cfg = config.clone();
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));

    let mut prepared: Vec<PreparedFile> = stream::iter(inputs.into_iter().map(|input| {
        let cfg = cfg.clone();
        let sem = sem.clone();
        async move {
            let permit = sem.acquire().await.expect("semaphore closed");
            // Note: We pass None for embedder here because we want to batch embed globally later
            let res = tokio::task::spawn_blocking(move || prepare_file(&input, &cfg, None))
                .await
                .ok()
                .and_then(|r| r.ok());
            drop(permit);
            res
        }
    }))
    .buffer_unordered(concurrency.max(1))
    .filter_map(|r| async move { r })
    .collect()
    .await;

    // Global batch embedding
    if let Some(embedder) = embedder {
        // Collect all chunks that need embedding
        let mut all_chunks_refs: Vec<&mut coderet_core::models::Chunk> = Vec::new();
        for file in &mut prepared {
            for chunk in &mut file.chunks {
                all_chunks_refs.push(chunk);
            }
        }

        if !all_chunks_refs.is_empty() {
            let total_chunks = all_chunks_refs.len();
            println!("Embedding {} chunks in batches...", total_chunks);

            let texts: Vec<String> = all_chunks_refs.iter().map(|c| c.content.clone()).collect();
            // Embed in batches of 512 to avoid hitting API limits too hard
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

    prepared
}

pub fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

fn prepare_file(
    input: &FileInput,
    config: &Config,
    _embedder: Option<Arc<dyn Embedder + Send + Sync>>,
) -> Result<PreparedFile> {
    use coderet_core::chunking::GenericChunker;
    let chunker = GenericChunker::with_config(input.language.clone(), config.chunking.clone());
    let mut chunks = chunker.chunk(&input.content, &input.path)?;
    for chunk in chunks.iter_mut() {
        if chunk.content_hash.is_empty() {
            chunk.content_hash = compute_hash(&chunk.content);
        }
    }

    // Embedding is now handled globally in prepare_files_async
    // if let Some(embedder) = embedder { ... } removed

    let mut symbols: Vec<coderet_core::models::Symbol> = Vec::new();
    let mut chunk_symbol_edges: Vec<(String, String)> = Vec::new();
    if let Ok(syms) = extract_symbols(&input.content, &input.path, &input.language) {
        for sym in syms {
            if let Some(chunk_id) = find_covering_chunk_id(&chunks, sym.start_line, sym.end_line) {
                chunk_symbol_edges.push((chunk_id.clone(), sym.id.clone()));
            }
            symbols.push(sym);
        }
    }

    let (mut calls, mut imports) = extract_calls_imports(&input.language, &input.content);
    if calls.is_empty() && imports.is_empty() {
        let (fallback_calls, fallback_imports) = extract_calls_and_imports(&input.content);
        calls.extend(
            fallback_calls
                .into_iter()
                .map(|name| RelationRef { name, line: 1 }),
        );
        imports.extend(
            fallback_imports
                .into_iter()
                .map(|name| RelationRef { name, line: 1 }),
        );
    }

    let mut call_edges: Vec<(String, String)> = Vec::new();
    let mut import_edges: Vec<(String, String)> = Vec::new();

    for c in calls {
        let source_chunk = find_covering_chunk_id(&chunks, c.line, c.line)
            .unwrap_or_else(|| input.file_node_id.clone());
        let caller_node = chunk_symbol_edges
            .iter()
            .find_map(|(cid, sid)| {
                if cid == &source_chunk {
                    Some(sid.clone())
                } else {
                    None
                }
            })
            .unwrap_or(source_chunk.clone());
        println!(
            "DEBUG(prepare_file): Processing call: name='{}', line={}, source_chunk='{}', caller_node='{}'",
            c.name, c.line, source_chunk, caller_node
        );
        call_edges.push((caller_node, c.name));
    }
    for imp in imports {
        let source_chunk = find_covering_chunk_id(&chunks, imp.line, imp.line)
            .unwrap_or_else(|| input.file_node_id.clone());
        let caller_node = chunk_symbol_edges
            .iter()
            .find_map(|(cid, sid)| {
                if cid == &source_chunk {
                    Some(sid.clone())
                } else {
                    None
                }
            })
            .unwrap_or(source_chunk.clone());
        import_edges.push((caller_node, imp.name));
    }

    Ok(PreparedFile {
        path: input.path.clone(),
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
    chunks: &[coderet_core::models::Chunk],
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

/// Fallback regex-based extractor for calls/imports when tree-sitter gives nothing.
fn extract_calls_and_imports(content: &str) -> (Vec<String>, Vec<String>) {
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    let call_re = regex::Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();
    let rust_import = regex::Regex::new(r"^\s*use\s+([A-Za-z0-9_:]+)").unwrap();
    let py_import = regex::Regex::new(
        r"^\s*(?:from\s+([A-Za-z0-9_\.]+)\s+import\s+([A-Za-z0-9_]+)|import\s+([A-Za-z0-9_\.]+))",
    )
    .unwrap();
    let go_import = regex::Regex::new(r#"^\s*import\s+"([^"]+)""#).unwrap();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("#") {
            continue;
        }

        if let Some(cap) = rust_import.captures(line) {
            if let Some(name) = cap.get(1) {
                if let Some(last) = name.as_str().rsplit("::").next() {
                    imports.push(last.to_string());
                }
            }
        }
        if let Some(cap) = py_import.captures(line) {
            if let Some(n) = cap.get(2) {
                imports.push(n.as_str().to_string());
            } else if let Some(n) = cap.get(3) {
                if let Some(last) = n.as_str().rsplit('.').next() {
                    imports.push(last.to_string());
                }
            } else if let Some(n) = cap.get(1) {
                imports.push(n.as_str().to_string());
            }
        }
        if let Some(cap) = go_import.captures(line) {
            if let Some(n) = cap.get(1) {
                if let Some(last) = n.as_str().rsplit('/').last() {
                    imports.push(last.to_string());
                }
            }
        }

        for cap in call_re.captures_iter(line) {
            if let Some(name) = cap.get(1) {
                calls.push(name.as_str().to_string());
            }
        }
    }

    (calls, imports)
}
