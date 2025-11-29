mod models;

use anyhow::Result;
pub use models::{ChunkRecord, FileRecord, SymbolRecord, SurrealGraphNode, SurrealGraphEdge, CommitLogRecord};
use std::path::Path;
use surrealdb::engine::local::RocksDb;
use surrealdb::Surreal;
use surrealdb::sql::Thing;

#[derive(Clone)]
pub struct SurrealStore {
    db: Surreal<surrealdb::engine::local::Db>,
}

impl SurrealStore {
    pub async fn new(path: &Path) -> Result<Self> {
        let db = Surreal::new::<RocksDb>(path).await?;
        db.use_ns("emry").use_db("main").await?;
        
        // Initialize Schema
        Self::init_schema(&db).await?;
        
        Ok(Self { db })
    }

    async fn init_schema(db: &Surreal<surrealdb::engine::local::Db>) -> Result<()> {
        // Vector Index
        db.query("DEFINE INDEX chunk_embedding ON chunk FIELDS embedding HNSW DIMENSION 1536 DIST COSINE M 16 EFC 64").await?;
        
        // Full-Text Index
        db.query("DEFINE ANALYZER code_analyzer TOKENIZERS class, blank FILTERS lowercase, ascii, snowball(english)").await?;
        db.query("DEFINE INDEX chunk_content ON chunk FIELDS content SEARCH ANALYZER code_analyzer BM25").await?;
        
        // Unique Edge Indexes
        db.query("DEFINE INDEX unique_calls ON TABLE calls COLUMNS in, out UNIQUE").await?;
        db.query("DEFINE INDEX unique_imports ON TABLE imports COLUMNS in, out UNIQUE").await?;
        db.query("DEFINE INDEX unique_defines ON TABLE defines COLUMNS in, out UNIQUE").await?;
        db.query("DEFINE INDEX unique_contains ON TABLE contains COLUMNS in, out UNIQUE").await?;
        
        Ok(())
    }
    
    pub fn db(&self) -> &Surreal<surrealdb::engine::local::Db> {
        &self.db
    }

    pub async fn add_commit(&self, commit_id: String, timestamp: u64, note: String) -> Result<()> {
        let record = CommitLogRecord {
            id: None,
            commit_id,
            timestamp,
            note,
        };
        let _: Vec<CommitLogRecord> = self.db.insert("commit_log").content(record).await?;
        Ok(())
    }

    pub async fn list_commits(&self, limit: usize) -> Result<Vec<CommitLogRecord>> {
        let mut res = self.db.query("SELECT * FROM commit_log ORDER BY timestamp DESC LIMIT $limit")
            .bind(("limit", limit))
            .await?;
        let commits: Vec<CommitLogRecord> = res.take(0)?;
        Ok(commits)
    }

    pub async fn add_file(
        &self,
        file: FileRecord,
        chunks: Vec<ChunkRecord>,
        symbols: Vec<SymbolRecord>,
        call_edges: Vec<(String, String)>,
    ) -> Result<()> {
        // Transactional write logic (simplified for now)
        
        // 1. Upsert File
        // eprintln!("Upserting file: {}", file.path);
        let mut file_content = file.clone();
        file_content.id = None;
        let _: Option<FileRecord> = self.db.upsert(("file", &file.path))
            .content(file_content)
            .await?;
            
        // 2. Upsert Chunks
        for chunk in chunks {
            if let Some(id) = &chunk.id {
                 // id is a Thing, which has .tb (table) and .id (id)
                 // We can construct a tuple ("table", "id_str")
                 let id_str = match &id.id {
                     surrealdb::sql::Id::String(s) => s.clone(),
                     _ => id.id.to_string(),
                 };
                 let mut chunk_content = chunk.clone();
                 chunk_content.id = None;
                 let _: Option<ChunkRecord> = self.db.upsert((id.tb.as_str(), id_str)).content(chunk_content).await?;
            } else {
                 let _: Vec<ChunkRecord> = self.db.insert("chunk").content(vec![chunk]).await?;
            }
        }
        
        // 3. Upsert Symbols
        for sym in &symbols {
            if let Some(id) = &sym.id {
                 let id_str = match &id.id {
                     surrealdb::sql::Id::String(s) => s.clone(),
                     _ => id.id.to_string(),
                 };
                 let mut sym_content = sym.clone();
                 sym_content.id = None;
                 let _: Option<SymbolRecord> = self.db.upsert((id.tb.as_str(), id_str)).content(sym_content).await?;
            } else {
                 let _: Vec<SymbolRecord> = self.db.insert("symbol").content(vec![sym.clone()]).await?;
            }
        }
        
        // 4. Edges (Defines)
        for sym in &symbols {
            let _ = self.db.query("RELATE $file->defines->$symbol")
                .bind(("file", file.id.clone()))
                .bind(("symbol", sym.id.clone()))
                .await;
        }

        // 5. Edges (Calls)
        // 5. Edges (Calls)
        for (caller_id, callee_name) in call_edges {
            // caller_id is a node ID (string), callee_name is a string name (potentially qualified)
            
            let (name, qualifier) = if let Some(idx) = callee_name.rfind("::") {
                // Rust style: mod::func
                (&callee_name[idx+2..], Some(&callee_name[..idx]))
            } else if let Some(idx) = callee_name.rfind('.') {
                 // Dot style: mod.func
                 (&callee_name[idx+1..], Some(&callee_name[..idx]))
            } else {
                (callee_name.as_str(), None)
            };

            // 1. Try to find specific symbol matching name AND qualifier (if present)
            // We fetch candidates with the same name
            let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                .bind(("name", name.to_string()))
                .await?;
            let candidates: Vec<SurrealGraphNode> = res.take(0)?;
            
            if name == "foo" {
                // println!("DEBUG: Resolving call to 'foo'. Qualifier: {:?}. Candidates: {}", qualifier, candidates.len());
            }

            let target_node = if let Some(qual) = qualifier {
                // Filter candidates where file path contains the qualifier
                // e.g. qual="std::fs", file_path=".../std/fs.rs" or similar
                // Simple heuristic: check if qualifier parts are in file path
                // For now, just check if file_path ends with qualifier (converted to path) or contains it
                
                // Normalize qualifier to path separators
                let qual_path = qual.replace("::", "/").replace('.', "/");
                
                candidates.iter().find(|c| c.file_path.contains(&qual_path)).cloned()
                    .or_else(|| {
                        // Fallback: if no qualified match, maybe just pick the first one? 
                        // Or maybe the qualifier was an alias?
                        // For now, if we have a qualifier but no match, we might want to be conservative
                        // But let's fallback to first candidate to maintain previous behavior if resolution fails
                        if name == "foo" {
                            // println!("DEBUG: No qualified match found for '{}' with path '{}'", name, qual_path);
                        }
                        candidates.first().cloned()
                    })
            } else {
                // No qualifier, pick first candidate (ambiguous)
                candidates.first().cloned()
            };
            
            if let Some(target) = target_node {
                 if name == "foo" {
                     // println!("DEBUG: Linked to target: {:?}", target.id);
                 }
                 let _ = self.db.query("RELATE $from->calls->$to")
                    .bind(("from", surrealdb::sql::thing(&caller_id)?))
                    .bind(("to", target.id))
                    .await;
            } else {
                 if name == "foo" {
                     // println!("DEBUG: No target found for 'foo'");
                 }
            }
        }
        
        Ok(())
    }

    pub async fn search_vector(&self, embedding: Vec<f32>, limit: usize) -> Result<Vec<ChunkRecord>> {
        let results: Vec<ChunkRecord> = self.db.query("SELECT * FROM chunk WHERE embedding <|10, cosine|> $query_vec LIMIT $limit")
            .bind(("query_vec", embedding))
            .bind(("limit", limit))
            .await?
            .take(0)?;
        Ok(results)
    }

    pub async fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<ChunkRecord>> {
        let results: Vec<ChunkRecord> = self.db.query("SELECT * FROM chunk WHERE content @1@ $query LIMIT $limit")
            .bind(("query", query.to_string()))
            .bind(("limit", limit))
            .await?
            .take(0)?;
        Ok(results)
    }

    /// Pass 1: Add nodes (File, Chunk, Symbol)
    pub async fn add_file_nodes(
        &self,
        file: &FileRecord,
        chunks: &[ChunkRecord],
        symbols: &[SymbolRecord],
        chunk_to_symbol: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        // 1. Delete old data for this file
        // Use the file path directly as the ID
        let file_id_str = &file.path;
        
        // Delete existing file record and its relations
        let _: Option<FileRecord> = self.db.delete(("file", file_id_str)).await?;
        
        // Delete chunks for this file (they reference the file Thing)
        let file_thing = Thing::from(("file", file_id_str.as_str()));
        let _ = self.db.query("DELETE chunk WHERE file = $file")
            .bind(("file", file_thing.clone()))
            .await?;
            
        // Delete symbols for this file
        let _ = self.db.query("DELETE symbol WHERE file = $file")
            .bind(("file", file_thing.clone()))
            .await?;
            
        // 2. Create File record
        let mut file_content = file.clone();
        file_content.id = None;
        let _: Option<FileRecord> = self.db.upsert(("file", file_id_str))
            .content(file_content)
            .await?;
            
        // 3. Create Chunks
        for chunk in chunks {
            if let Some(id) = &chunk.id {
                // Extract the raw string ID from the Thing
                let id_str = id.id.to_string();
                let mut chunk_content = chunk.clone();
                chunk_content.id = None;
                let _: Option<ChunkRecord> = self.db.upsert(("chunk", id_str))
                    .content(chunk_content)
                    .await?;
            }
        }
        
        // 4. Create Symbols
        // Build symbol ID from file path and symbol name (matches service.rs line 145-146)
        // Format: "symbol:/path/to/file.rs::symbol_name"
        for symbol in symbols {
            // Extract file path from the file Thing reference
            let file_path = match &symbol.file.id {
                surrealdb::sql::Id::String(s) => s.clone(),
                _ => symbol.file.id.to_string(),
            };
            
            // Construct the simple ID: "file_path::symbol_name"
            let symbol_id_str = format!("{}::{}", file_path, symbol.name);
            
            let mut symbol_content = symbol.clone();
            symbol_content.id = None;
            let _: Option<SymbolRecord> = self.db.upsert(("symbol", symbol_id_str))
                .content(symbol_content)
                .await?;
        }
        
        // 5. Link Chunks to Symbols (Contains relation)
        // This is intra-file, so we can do it in pass 1
        for (chunk_id, symbol_id) in chunk_to_symbol {
             let _ = self.db.query("RELATE $from->contains->$to")
                .bind(("from", surrealdb::sql::thing(symbol_id)?))
                .bind(("to", surrealdb::sql::thing(chunk_id)?))
                .await;
        }

        // 6. Link File to Symbols (Defines relation)
        for symbol in symbols {
            // Extract file path from the file Thing reference
            let file_path = match &symbol.file.id {
                surrealdb::sql::Id::String(s) => s.clone(),
                _ => symbol.file.id.to_string(),
            };
            
            // Construct the simple ID: "file_path::symbol_name"
            let symbol_id_str = format!("{}::{}", file_path, symbol.name);
            let symbol_thing = Thing::from(("symbol", symbol_id_str.as_str()));

            let _ = self.db.query("RELATE $from->defines->$to")
                .bind(("from", symbol.file.clone()))
                .bind(("to", symbol_thing))
                .await;
        }
        
        Ok(())
    }

    /// Pass 2: Add edges (Calls, Imports)
    pub async fn add_file_edges(
        &self,
        call_edges: &[(String, String)],
        import_edges: &[(String, String)],
    ) -> Result<()> {
        // 1. Add Call Edges
        for (caller_id, callee_name) in call_edges {
            // Split callee_name into (qualifier, name)
            // e.g. "std::fs::read" -> (Some("std::fs"), "read")
            // e.g. "read" -> (None, "read")
            let (qualifier, name) = if let Some(idx) = callee_name.rfind("::") {
                (Some(&callee_name[0..idx]), &callee_name[idx+2..])
            } else if let Some(idx) = callee_name.rfind('.') {
                (Some(&callee_name[0..idx]), &callee_name[idx+1..])
            } else {
                (None, callee_name.as_str())
            };
            
            // Try to find specific symbol matching name AND qualifier (if present)
            let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                .bind(("name", name.to_string()))
                .await?;
            
            #[derive(Debug, serde::Deserialize)]
            struct DebugGraphNode {
                id: surrealdb::sql::Thing,
                label: Option<String>,
                kind: Option<String>,
                file_path: Option<String>,
            }

            let candidates: Vec<DebugGraphNode> = res.take(0)?;
            
            // eprintln!("DEBUG add_file_edges: Looking for '{}' (qual: {:?}) - found {} candidates", name, qualifier, candidates.len());
            
            let valid_candidates: Vec<SurrealGraphNode> = candidates.into_iter().filter_map(|c| {
                if let (Some(label), Some(kind), Some(file_path)) = (c.label.clone(), c.kind.clone(), c.file_path.clone()) {
                   Some(SurrealGraphNode {
                        id: c.id,
                        label,
                        kind,
                        file_path,
                    })
                } else {
                    // eprintln!("DEBUG: Skipping invalid node for {}: {:?}", name, c);
                    None
                }
            }).collect();
            
            // eprintln!("DEBUG add_file_edges: {} valid candidates for '{}'", valid_candidates.len(), name);
            // for vc in &valid_candidates {
            //     eprintln!("  Candidate: {} @ {}", vc.label, vc.file_path);
            // }
            
            let target_node = if let Some(qual) = qualifier {
                let qual_path = qual.replace("::", "/").replace('.', "/");
                
                // eprintln!("DEBUG add_file_edges: Looking for qualifier path: {}", qual_path);
                
                valid_candidates.iter().find(|c| c.file_path.contains(&qual_path)).cloned()
                    .or_else(|| {
                        valid_candidates.first().cloned()
                    })
            } else {
                valid_candidates.first().cloned()
            };
            
            if let Some(target) = target_node {
                 // eprintln!("DEBUG add_file_edges: Creating edge {} -> {}", caller_id, target.id);
                 let _ = self.db.query("RELATE $from->calls->$to")
                    .bind(("from", surrealdb::sql::thing(caller_id)?))
                    .bind(("to", target.id))
                    .await;
            } else {
                 // eprintln!("DEBUG add_file_edges: No target found for '{}'", callee_name);
            }
        }
        
        // 2. Add Import Edges
        for (importer_id, imported_name) in import_edges {
             // For imports, we might want to link to a file or a module symbol
             // For now, let's try to find a symbol with that name
             // Similar logic to calls, but maybe less strict on qualifier?
             // Or maybe imports ARE the qualifier?
             // Let's keep it simple: find any symbol with that name
             
             // TODO: Enhance import resolution
             let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name LIMIT 1")
                .bind(("name", imported_name.to_string()))
                .await?;
             let target: Option<SurrealGraphNode> = res.take(0)?;
             
             if let Some(t) = target {
                 let _ = self.db.query("RELATE $from->imports->$to")
                    .bind(("from", surrealdb::sql::thing(importer_id)?))
                    .bind(("to", t.id))
                    .await;
             }
        }
        
        Ok(())
    }
    pub async fn delete_file(&self, path: &str) -> Result<()> {
        let file_thing = surrealdb::sql::Thing::from(("file", path));
        
        // Delete File
        let _: Option<FileRecord> = self.db.delete(("file", path)).await?;
        
        // Delete Chunks
        let _ = self.db.query("DELETE chunk WHERE file = $file")
            .bind(("file", file_thing.clone()))
            .await?;
            
        // Delete Symbols
        let _ = self.db.query("DELETE symbol WHERE file = $file")
            .bind(("file", file_thing))
            .await?;
            
        Ok(())
    }

    pub async fn add_graph_edge(&self, from: (String, String), to: (String, String), relation: &str) -> Result<()> {
        let res = self.db.query(format!("RELATE $from->{}->$to", relation))
            .bind(("from", from))
            .bind(("to", to))
            .await;
            
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("already exists") {
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub async fn get_node(&self, id: &str) -> Result<Option<SurrealGraphNode>> {
        let thing = surrealdb::sql::thing(id)?;
        self.get_node_by_thing(&thing).await
    }

    pub async fn get_node_by_thing(&self, thing: &surrealdb::sql::Thing) -> Result<Option<SurrealGraphNode>> {
        let table = &thing.tb;
        
        let sql = match table.as_str() {
            "symbol" => "SELECT id, name as label, kind, file.path as file_path FROM $id",
            "file" => "SELECT id, path as label, 'file' as kind, path as file_path FROM $id",
            "chunk" => "SELECT id, 'chunk' as label, 'chunk' as kind, file.path as file_path FROM $id",
            _ => return Ok(None),
        };
        
        let mut res = self.db.query(sql).bind(("id", thing.clone())).await?;
        let node: Option<SurrealGraphNode> = res.take(0)?;
        Ok(node)
    }

    pub async fn find_nodes_by_label(&self, label: &str, file_filter: Option<&str>) -> Result<Vec<SurrealGraphNode>> {
        // Search symbols and files
        let mut nodes = Vec::new();
        
        // Symbols - with optional file filter
        let symbol_query = if file_filter.is_some() {
            "SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name CONTAINS $label AND file.path CONTAINS $filter LIMIT 10"
        } else {
            "SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name CONTAINS $label LIMIT 10"
        };
        
        let mut res = if let Some(filter) = file_filter {
            self.db.query(symbol_query)
                .bind(("label", label.to_string()))
                .bind(("filter", filter.to_string()))
                .await?
        } else {
            self.db.query(symbol_query)
                .bind(("label", label.to_string()))
                .await?
        };
        let symbols: Vec<SurrealGraphNode> = res.take(0)?;
        nodes.extend(symbols);
        
        // Files - with optional file filter
        let file_query = if file_filter.is_some() {
            "SELECT id, path as label, 'file' as kind, path as file_path FROM file WHERE path CONTAINS $label AND path CONTAINS $filter LIMIT 10"
        } else {
            "SELECT id, path as label, 'file' as kind, path as file_path FROM file WHERE path CONTAINS $label LIMIT 10"
        };
        
        let mut res = if let Some(filter) = file_filter {
            self.db.query(file_query)
                .bind(("label", label.to_string()))
                .bind(("filter", filter.to_string()))
                .await?
        } else {
            self.db.query(file_query)
                .bind(("label", label.to_string()))
                .await?
        };
        let files: Vec<SurrealGraphNode> = res.take(0)?;
        nodes.extend(files);
        
        Ok(nodes)
    }

    pub async fn get_neighbors(&self, id: &str, direction: &str) -> Result<Vec<SurrealGraphEdge>> {
        let thing = surrealdb::sql::thing(id)?;
        
        // Simplified approach: Just return edges with IDs.
        let sql = match direction {
            "out" => "SELECT in as source, out as target, type::table(id) as relation FROM $id->?",
            "in" => "SELECT in as source, out as target, type::table(id) as relation FROM $id<-?",
            _ => return Ok(Vec::new()),
        };

        let mut res = self.db.query(sql).bind(("id", thing)).await?;
        let edges: Vec<SurrealGraphEdge> = res.take(0)?;
        Ok(edges)
    }

    pub async fn list_all_symbols(&self) -> Result<Vec<SurrealGraphNode>> {
        // Fetch all symbols with their file paths
        let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol")
            .await?;
        let symbols: Vec<SurrealGraphNode> = res.take(0)?;
        Ok(symbols)
    }

    pub async fn list_files(&self) -> Result<Vec<FileRecord>> {
        let mut res = self.db.query("SELECT * FROM file").await?;
        let files: Vec<FileRecord> = res.take(0)?;
        Ok(files)
    }

    pub async fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        let mut res = self.db.query("SELECT * FROM file WHERE path = $path LIMIT 1")
            .bind(("path", path.to_string()))
            .await?;
        let file: Option<FileRecord> = res.take(0)?;
        Ok(file)
    }

    pub async fn count_files(&self) -> Result<usize> {
        let mut res = self.db.query("SELECT count() FROM file GROUP ALL").await?;
        let result: Option<serde_json::Value> = res.take(0)?;
        if let Some(val) = result {
            if let Some(count) = val.get("count") {
                 return Ok(count.as_u64().unwrap_or(0) as usize);
            }
        }
        Ok(0)
    }
}
