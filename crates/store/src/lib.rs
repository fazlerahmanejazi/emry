mod models;

use anyhow::Result;
pub use models::{ChunkRecord, FileRecord, SymbolRecord, SurrealGraphNode, SurrealGraphEdge, CommitLogRecord};
use emry_core::relations::RelationRef;
use std::path::Path;
use surrealdb::engine::local::RocksDb;
use surrealdb::Surreal;
use surrealdb::sql::Thing;

#[derive(Clone)]
pub struct SurrealStore {
    db: Surreal<surrealdb::engine::local::Db>,
}

impl SurrealStore {
    pub async fn new(path: &Path, vector_dimension: usize) -> Result<Self> {
        let db = Surreal::new::<RocksDb>(path).await?;
        db.use_ns("emry").use_db("main").await?;
        
        // Initialize Schema
        Self::init_schema(&db, vector_dimension).await?;
        
        Ok(Self { db })
    }

    async fn init_schema(db: &Surreal<surrealdb::engine::local::Db>, vector_dimension: usize) -> Result<()> {
        let query = format!("DEFINE INDEX chunk_embedding ON chunk FIELDS embedding HNSW DIMENSION {} DIST COSINE M 16 EFC 64", vector_dimension);
        db.query(query).await?;
        
        db.query("DEFINE ANALYZER code_analyzer TOKENIZERS class, blank FILTERS lowercase, ascii, snowball(english)").await?;
        db.query("DEFINE INDEX chunk_content ON chunk FIELDS content SEARCH ANALYZER code_analyzer BM25").await?;
        
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
        let mut file_content = file.clone();
        file_content.id = None;
        let _: Option<FileRecord> = self.db.upsert(("file", &file.path))
            .content(file_content)
            .await?;
            
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
        
        for sym in &symbols {
            let _ = self.db.query("RELATE $file->defines->$symbol")
                .bind(("file", file.id.clone()))
                .bind(("symbol", sym.id.clone()))
                .await;
        }
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

            let target_node = if let Some(qual) = qualifier {
                // Filter candidates where file path contains the qualifier
                let qual_path = qual.replace("::", "/").replace('.', "/");
                
                candidates.iter()
                    .find(|c| c.file_path.contains(&qual_path))
                    .cloned()
                    .or_else(|| candidates.first().cloned())
            } else {
                candidates.first().cloned()
            };
            
            if let Some(target) = target_node {
                 let _ = self.db.query("RELATE $from->calls->$to")
                    .bind(("from", surrealdb::sql::thing(&caller_id)?))
                    .bind(("to", target.id))
                    .await;
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

        // 7. Link Parent Symbols to Child Symbols (Hierarchy)
        for symbol in symbols {
            if let Some(parent_name) = &symbol.parent_scope {
                let file_path = match &symbol.file.id {
                    surrealdb::sql::Id::String(s) => s.clone(),
                    _ => symbol.file.id.to_string(),
                };
                
                let parent_id_str = format!("{}::{}", file_path, parent_name);
                let parent_thing = Thing::from(("symbol", parent_id_str.as_str()));
                
                let child_id_str = format!("{}::{}", file_path, symbol.name);
                let child_thing = Thing::from(("symbol", child_id_str.as_str()));

                let _ = self.db.query("RELATE $from->defines->$to")
                    .bind(("from", parent_thing))
                    .bind(("to", child_thing))
                    .await;
            }
        }
        
        Ok(())
    }

    /// Prioritize symbol candidates based on proximity to the caller.
    /// Priority order:
    /// 1. Same file (exact match)
    /// 2. Same directory
    /// 3. Parent directory
    /// 4. First match (arbitrary fallback)
    fn prioritize_candidate(
        candidates: &[SurrealGraphNode],
        caller_id: &str,
    ) -> Option<SurrealGraphNode> {
        if candidates.is_empty() {
            return None;
        }
        
        // Extract caller file path from ID
        let caller_file = Self::extract_file_from_id(caller_id);
        
        // 1. Same file (highest priority)
        if let Some(caller_path) = &caller_file {
            if let Some(c) = candidates.iter().find(|c| &c.file_path == caller_path) {
                return Some(c.clone());
            }
        }
        
        // 2. Same directory
        if let Some(caller_path) = &caller_file {
            if let Some(caller_dir) = std::path::Path::new(caller_path).parent() {
                let caller_dir_str = caller_dir.to_string_lossy();
                if let Some(c) = candidates.iter().find(|c| {
                    std::path::Path::new(&c.file_path)
                        .parent()
                        .map(|p| p.to_string_lossy() == caller_dir_str)
                        .unwrap_or(false)
                }) {
                    return Some(c.clone());
                }
            }
        }
        
        // 3. Parent directory (one level up)
        if let Some(caller_path) = &caller_file {
            if let Some(caller_dir) = std::path::Path::new(caller_path).parent() {
                if let Some(caller_parent) = caller_dir.parent() {
                    let parent_str = caller_parent.to_string_lossy();
                    if let Some(c) = candidates.iter().find(|c| {
                        c.file_path.starts_with(parent_str.as_ref())
                    }) {
                        return Some(c.clone());
                    }
                }
            }
        }
        
        // 4. Fallback: first match
        candidates.first().cloned()
    }

    /// Extract file path from node ID.
    /// Examples:
    /// - "symbol:⟨file_path::symbol_name⟩" -> Some("file_path")
    /// - "symbol:file_path::symbol_name" -> Some("file_path")
    /// - "chunk:uuid" -> None (chunks don't have predictable file info in ID)
    fn extract_file_from_id(id: &str) -> Option<String> {
        if let Some(rest) = id.strip_prefix("symbol:") {
            // Handle angle bracket format: ⟨filepath::name⟩
            let content = if rest.starts_with('⟨') && rest.ends_with('⟩') {
                &rest[3..rest.len()-3] // Remove ⟨ and ⟩ (3 bytes each in UTF-8)
            } else {
                rest
            };
            
            // Format: "file_path::symbol_name"
            if let Some(idx) = content.rfind("::") {
                return Some(content[..idx].to_string());
            }
        }
        None
    }

    pub async fn add_file_edges(
        &self,
        call_edges: &[(String, RelationRef)],
        import_edges: &[(String, RelationRef)],
    ) -> Result<()> {
        // 1. Build Local Scope Map from Imports
        // Map: local_name -> full_import_path
        let mut scope_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        
        for (_, relation) in import_edges {
            if let Some(alias) = &relation.alias {
                // "import x as y" -> local="y", full="x"
                scope_map.insert(alias.clone(), relation.name.clone());
            } else {
                // "import x.y.z" -> local="z", full="x.y.z"
                let name = &relation.name;
                let local_name = if let Some(idx) = name.rfind("::") {
                    &name[idx+2..]
                } else if let Some(idx) = name.rfind('.') {
                    &name[idx+1..]
                } else if let Some(idx) = name.rfind('/') {
                    &name[idx+1..]
                } else {
                    name.as_str()
                };
                scope_map.insert(local_name.to_string(), name.clone());
            }
        }
        
        // 2. Add Call Edges with Polyglot Resolution
        for (caller_id, call) in call_edges {
            let name = &call.name;
            let context = &call.context; // e.g., "obj" in "obj.method()"

            // RESOLUTION STRATEGY:
            // 1. Context Resolution: If context exists, try to map it to a module/type.
            // 2. Scope Resolution: If name is in scope, use full path.
            // 3. Global Search: Fallback.

            let target_node = if let Some(ctx) = context {
                // Case A: Method call on an object/module (ctx.name())
                
                // Check if context is an alias in scope
                // e.g. import mod as m; m.func() -> ctx="m", maps to "mod"
                if let Some(full_module_path) = scope_map.get(ctx) {
                    // We are looking for symbol 'name' in module 'full_module_path'
                    // Query: name='name', file_path contains 'full_module_path'
                    
                    let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                        .bind(("name", name.to_string()))
                        .await?;
                    let candidates: Vec<SurrealGraphNode> = res.take(0)?;
                    
                    // Normalize path separators for matching
                let mod_path_slash = if full_module_path.contains('/') {
                    full_module_path.to_string()
                } else {
                    full_module_path.replace("::", "/").replace('.', "/")
                };
                
                candidates.iter().find(|c| c.file_path.contains(&mod_path_slash)).cloned()
                    .or_else(|| Self::prioritize_candidate(&candidates, caller_id))
                } else {
                    // Context is not an import alias. It might be a variable or a direct module name.
                    // e.g. "std::fs::read()" -> ctx="std::fs" (if parser split it) or just name="std::fs::read"
                    // Or "x.method()" where x is a local variable.
                    
                    // Try to find 'name' globally, filtering by context in file path
                    let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                        .bind(("name", name.to_string()))
                        .await?;
                    let candidates: Vec<SurrealGraphNode> = res.take(0)?;
                    
                let ctx_slash = if ctx.contains('/') {
                    ctx.to_string()
                } else {
                    ctx.replace("::", "/").replace('.', "/")
                };
                candidates.iter().find(|c| c.file_path.contains(&ctx_slash)).cloned()
                     .or_else(|| Self::prioritize_candidate(&candidates, caller_id))
                }
            } else if let Some(full_path) = scope_map.get(name) {
                // Case B: Direct call to imported symbol (name())
                
                // Extract symbol name from full path (last part)
                let symbol_part = if let Some(idx) = full_path.rfind("::") {
                    &full_path[idx+2..]
                } else if let Some(idx) = full_path.rfind('.') {
                    if full_path.contains('/') {
                         if let Some(idx) = full_path.rfind('/') {
                             &full_path[idx+1..]
                         } else {
                             full_path.as_str()
                         }
                    } else {
                        &full_path[idx+1..]
                    }
                } else {
                    full_path.as_str()
                };
                
                // Extract module part (everything before)
                let module_part = if let Some(idx) = full_path.rfind("::") {
                    &full_path[..idx]
                } else if let Some(idx) = full_path.rfind('.') {
                     if full_path.contains('/') {
                         if let Some(idx) = full_path.rfind('/') {
                             &full_path[..idx]
                         } else {
                             ""
                         }
                     } else {
                        &full_path[..idx]
                     }
                } else {
                    ""
                };
                
                let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                    .bind(("name", symbol_part.to_string()))
                    .await?;
                let candidates: Vec<SurrealGraphNode> = res.take(0)?;
                
                let mod_path_slash = if module_part.contains('/') {
                    module_part.to_string()
                } else {
                    module_part.replace("::", "/").replace('.', "/")
                };
                
                candidates.iter().find(|c| c.file_path.contains(&mod_path_slash)).cloned()
                    .or_else(|| Self::prioritize_candidate(&candidates, caller_id))
            } else {
                // Case C: Global Search (No context, not in scope)
                // e.g. "print()" or implicit global
                
                let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                    .bind(("name", name.to_string()))
                    .await?;
                let candidates: Vec<SurrealGraphNode> = res.take(0)?;
                
                // Priority: same file > same directory > parent directory > first match
                Self::prioritize_candidate(&candidates, caller_id)
            };
            
            if let Some(target) = target_node {
                 let _ = self.db.query("RELATE $from->calls->$to")
                    .bind(("from", surrealdb::sql::thing(caller_id)?))
                    .bind(("to", target.id))
                    .await;
            }
        }
        
        // 3. Add Import Edges
        for (importer_id, relation) in import_edges {
             let full_path = &relation.name;
             
             // Extract symbol part for query
             let symbol_part = if let Some(idx) = full_path.rfind("::") {
                &full_path[idx+2..]
            } else if let Some(idx) = full_path.rfind('.') {
                 if full_path.contains('/') {
                     if let Some(idx) = full_path.rfind('/') {
                         &full_path[idx+1..]
                     } else {
                         full_path.as_str()
                     }
                 } else {
                    &full_path[idx+1..]
                 }
            } else {
                full_path.as_str()
            };
            
            // Extract module part for filtering
            let module_part = if let Some(idx) = full_path.rfind("::") {
                &full_path[..idx]
            } else if let Some(idx) = full_path.rfind('.') {
                 if full_path.contains('/') {
                     if let Some(idx) = full_path.rfind('/') {
                         &full_path[..idx]
                     } else {
                         ""
                     }
                 } else {
                    &full_path[..idx]
                 }
            } else {
                ""
            };
            
             let mut res = self.db.query("SELECT id, name as label, kind, file.path as file_path FROM symbol WHERE name = $name")
                .bind(("name", symbol_part.to_string()))
                .await?;
             let candidates: Vec<SurrealGraphNode> = res.take(0)?;
             
             let mod_path_slash = if module_part.contains('/') {
                module_part.to_string()
            } else {
                module_part.replace("::", "/").replace('.', "/")
            };
            
             let target = candidates.iter().find(|c| c.file_path.contains(&mod_path_slash)).cloned()
                .or_else(|| Self::prioritize_candidate(&candidates, importer_id));
             
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

    pub async fn get_chunk(&self, id: &str) -> Result<Option<ChunkRecord>> {
        let thing = surrealdb::sql::thing(id)?;
        let mut res = self.db.query("SELECT * FROM $id")
            .bind(("id", thing))
            .await?;
        let chunk: Option<ChunkRecord> = res.take(0)?;
        Ok(chunk)
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
