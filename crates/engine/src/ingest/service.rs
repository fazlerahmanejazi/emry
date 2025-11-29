use anyhow::Result;
use emry_core::chunking::{GenericChunker, Chunker};
use emry_core::models::Language;
use emry_core::symbols::extract_symbols;
use emry_core::traits::Embedder;
use emry_store::{SurrealStore, FileRecord, ChunkRecord, SymbolRecord};
use std::path::Path;
use std::sync::Arc;
use surrealdb::sql::Thing;
use super::pipeline::compute_hash;

pub struct IngestionService {
    store: Arc<SurrealStore>,
    embedder: Option<Arc<dyn Embedder + Send + Sync>>,
}

impl IngestionService {
    pub fn new(
        store: Arc<SurrealStore>,
        embedder: Option<Arc<dyn Embedder + Send + Sync>>,
    ) -> Self {
        Self { store, embedder }
    }

    pub async fn ingest_file(&self, path: &str, content: &str) -> Result<()> {
        let file_path = Path::new(path);
        let language = Language::from_extension(
            file_path.extension().and_then(|e| e.to_str()).unwrap_or("")
        );
        
        // 1. Chunking
        // We need a default config for now
        let chunking_config = emry_config::ChunkingConfig::default();
        let chunker = GenericChunker::with_config(language.clone(), chunking_config);
        let core_chunks = chunker.chunk(content, file_path)?;
        
        // 2. Embedding (Global/Batch usually, but here per file for simplicity in MVP)
        let mut chunks_with_embeddings = core_chunks.clone();
        if let Some(embedder) = &self.embedder {
            let texts: Vec<String> = core_chunks.iter().map(|c| c.content.clone()).collect();
            if let Ok(embeddings) = embedder.embed_batch(&texts).await {
                for (i, emb) in embeddings.into_iter().enumerate() {
                    if i < chunks_with_embeddings.len() {
                        chunks_with_embeddings[i].embedding = Some(emb);
                    }
                }
            }
        }
        
        // 3. Symbol Extraction
        let core_symbols = extract_symbols(content, file_path, &language).unwrap_or_default();
        
        // 4. Convert to Store Records
        let file_id = Thing::from(("file", path));
        
        let file_record = FileRecord {
            id: Some(file_id.clone()),
            path: path.to_string(),
            language: language.to_string(),
            content: content.to_string(),
            hash: compute_hash(content),
            last_modified: 0, // TODO
        };
        
        let chunk_records: Vec<ChunkRecord> = chunks_with_embeddings.into_iter().map(|c| {
            ChunkRecord {
                id: None, // Let DB generate UUID
                content: c.content,
                embedding: c.embedding.filter(|v| !v.is_empty()),
                file: file_id.clone(),
                start_line: c.start_line,
                end_line: c.end_line,
                scopes: c.scope_path,
            }
        }).collect();
        
        let symbol_records: Vec<SymbolRecord> = core_symbols.into_iter().map(|s| {
            SymbolRecord {
                id: Some(Thing::from(("symbol", format!("{}::{}", path, s.name).as_str()))),
                name: s.name,
                kind: s.kind,
                file: file_id.clone(),
                start_line: s.start_line,
                end_line: s.end_line,
            }
        }).collect();
        
        // 5. Save to Store
        self.store.add_file(file_record, chunk_records, symbol_records, Vec::new()).await?;
        
        Ok(())
    }

    /// Pass 1: Ingest nodes (File, Chunk, Symbol)
    pub async fn ingest_nodes(&self, file: super::pipeline::PreparedFile) -> Result<()> {
        let file_id_str = file.path.to_string_lossy().to_string();
        let file_id = Thing::from(("file", file_id_str.as_str()));

        // 1. Create File Record
        let file_record = FileRecord {
            id: Some(file_id.clone()),
            path: file.path.to_string_lossy().to_string(),
            language: file.language.to_string(),
            content: file.content.to_string(),
            hash: compute_hash(&file.content),
            last_modified: file.last_modified as i64,
        };

        // 2. Create Chunk Records
        // We need to embed chunks if they haven't been embedded yet (e.g. single file ingest)
        // But for pipeline, they are already embedded.
        let chunks_with_embeddings = if file.chunks.iter().any(|c| c.embedding.is_none()) {
             if let Some(embedder) = &self.embedder {
                 let core_chunks = file.chunks.clone();
                 let texts: Vec<String> = core_chunks.iter().map(|c| c.content.clone()).collect();
                 if let Ok(embeddings) = embedder.embed_batch(&texts).await {
                     core_chunks.into_iter().enumerate().map(|(i, mut c)| {
                         c.embedding = Some(embeddings[i].clone());
                         c
                     }).collect()
                 } else {
                     file.chunks.clone()
                 }
             } else {
                 file.chunks.clone()
             }
        } else {
            file.chunks.clone()
        };

        let chunk_records: Vec<ChunkRecord> = chunks_with_embeddings.into_iter().map(|c| {
            ChunkRecord {
                id: Some(Thing::from(("chunk", c.id.as_str()))),
                content: c.content,
                embedding: c.embedding.filter(|v| !v.is_empty()),
                file: file_id.clone(),
                start_line: c.start_line,
                end_line: c.end_line,
                scopes: c.scope_path,
            }
        }).collect();

        // 3. Create Symbol Records
        let symbol_records: Vec<SymbolRecord> = file.symbols.iter().map(|s| {
            let new_id_str = format!("{}::{}", file.path.to_string_lossy(), s.name);
            let new_id_thing = Thing::from(("symbol", new_id_str.as_str()));
            
            SymbolRecord {
                id: Some(new_id_thing),
                name: s.name.clone(),
                kind: s.kind.clone(),
                file: file_id.clone(),  // Use the simple path-based file ID
                start_line: s.start_line,
                end_line: s.end_line,
            }
        }).collect();
        
        // 4. Build ID map and Chunk->Symbol map
        let mut id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        
        // Map symbol IDs
        for s in &file.symbols {
            let new_id_str = format!("{}::{}", file.path.to_string_lossy(), s.name);
            let new_id_thing = Thing::from(("symbol", new_id_str.as_str()));
            id_map.insert(s.id.clone(), new_id_thing.to_string());
        }
        
        // Build chunk -> symbol map for hoisting
        let mut chunk_to_symbol = std::collections::HashMap::new();
        for (cid, sid) in &file.chunk_symbol_edges {
            if let Some(symbol_storage_id) = id_map.get(sid) {
                 // We need the chunk storage ID here
                 let chunk_thing = Thing::from(("chunk", cid.as_str()));
                 chunk_to_symbol.insert(chunk_thing.to_string(), symbol_storage_id.clone());
            }
        }

        self.store.add_file_nodes(
            &file_record,
            &chunk_records,
            &symbol_records,
            &chunk_to_symbol
        ).await?;
        
        Ok(())
    }
        



    /// Pass 2: Ingest edges (Calls, Imports)
    pub async fn ingest_edges(&self, file: super::pipeline::PreparedFile) -> Result<()> {
        let file_id_str = file.path.to_string_lossy().to_string();
        let _file_id = Thing::from(("file", file_id_str.as_str()));
        
        // We need to rebuild the ID map to translate edges
        // This is a bit redundant but safe. In a more optimized version we could cache this.
        let mut id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        
        // Map symbol IDs
        for s in &file.symbols {
            let new_id_str = format!("{}::{}", file.path.to_string_lossy(), s.name);
            let new_id_thing = Thing::from(("symbol", new_id_str.as_str()));
            id_map.insert(s.id.clone(), new_id_thing.to_string());
        }
        
        // Build chunk -> symbol map for hoisting
        let mut chunk_to_symbol = std::collections::HashMap::new();
        for (cid, sid) in &file.chunk_symbol_edges {
            if let Some(symbol_storage_id) = id_map.get(sid) {
                 let chunk_thing = Thing::from(("chunk", cid.as_str()));
                 chunk_to_symbol.insert(chunk_thing.to_string(), symbol_storage_id.clone());
            }
        }

        // Translate edges
        let translated_edges: Vec<(String, String)> = file.call_edges.into_iter().filter_map(|(caller, callee)| {
            // 1. Try to hoist: Check if caller is a chunk that belongs to a symbol
            if let Some(symbol_id) = chunk_to_symbol.get(&caller) {
                return Some((symbol_id.clone(), callee));
            }
            
            // 2. Fallback: Direct mapping (Symbol -> Symbol or Top-level Chunk -> Target)
            if let Some(new_caller) = id_map.get(&caller) {
                Some((new_caller.clone(), callee))
            } else {
                // If caller is not found in map, it might be the file itself or something else.
                // For now we skip untranslatable callers to avoid bad edges
                None
            }
        }).collect();

        // Translate import edges
        let translated_import_edges: Vec<(String, String)> = file.import_edges.into_iter().filter_map(|(importer, imported)| {
             // Importer is usually a symbol or file node ID from the pipeline
             
             if let Some(new_importer) = id_map.get(&importer) {
                 Some((new_importer.clone(), imported))
             } else if importer == file.file_node_id {
                 // If importer is the file itself, map to storage file ID
                 let file_thing = Thing::from(("file", file_id_str.as_str()));
                 Some((file_thing.to_string(), imported))
             } else {
                 None
             }
        }).collect();
        
        // eprintln!("DEBUG ingest_edges for {}: {} translated edges", file.path.display(), translated_edges.len());
        // for (caller, callee) in &translated_edges {
        //     eprintln!("  {} -> {}", caller, callee);
        // }
        
        self.store.add_file_edges(&translated_edges, &translated_import_edges).await?;
        Ok(())
    }
}
