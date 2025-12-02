use anyhow::Result;
use emry_core::traits::Embedder;
use emry_store::{SurrealStore, ChunkRecord};
use std::sync::Arc;

pub struct SearchService {
    store: Arc<SurrealStore>,
    embedder: Option<Arc<dyn Embedder + Send + Sync>>,
}

impl SearchService {
    pub fn store(&self) -> &Arc<SurrealStore> {
        &self.store
    }

    pub fn new(
        store: Arc<SurrealStore>,
        embedder: Option<Arc<dyn Embedder + Send + Sync>>,
    ) -> Self {
        Self { store, embedder }
    }

    pub async fn search(&self, query: &str, limit: usize, keywords: Option<&[String]>) -> Result<Vec<ChunkRecord>> {
        let mut results = Vec::new();
        
        // Vector Search
        if let Some(embedder) = &self.embedder {
            let embed_query = if let Some(kws) = keywords {
                format!("{} {}", query, kws.join(" "))
            } else {
                query.to_string()
            };

            if let Ok(embedding) = embedder.embed(&embed_query).await {
                match self.store.search_vector(embedding, limit).await {
                    Ok(vec_results) => {
                        // eprintln!("Vector search found {} results", vec_results.len());
                        results.extend(vec_results);
                    }
                    Err(e) => eprintln!("Vector search failed: {}", e),
                }
            }
        }
        
        // FTS
        let fts_query = if let Some(kws) = keywords {
            format!("{} {}", query, kws.join(" "))
        } else {
            query.to_string()
        };

        match self.store.search_fts(&fts_query, limit).await {
            Ok(fts_results) => {
                // eprintln!("FTS search found {} results", fts_results.len());
                results.extend(fts_results);
            }
            Err(e) => eprintln!("FTS search failed: {}", e),
        }
        
        results.sort_by(|a, b| a.id.cmp(&b.id));
        results.dedup_by(|a, b| a.id == b.id);
        
        Ok(results)
    }

    pub async fn search_with_context(&self, query: &str, limit: usize, keywords: Option<&[String]>) -> Result<emry_core::models::ContextGraph> {
        let mut anchors = self.search(query, limit, keywords).await?;
        let mut context_chunks = Vec::new();
        
        let mut related_files = Vec::new();
        let mut related_symbols = Vec::new();
        let mut edges = Vec::new();
        
        for anchor in &anchors {
            if let Some(anchor_id) = &anchor.id {
                let anchor_id_str = anchor_id.to_string();
                
                // Fetch File
                let file_thing = &anchor.file;
                if let Ok(Some(file_node)) = self.store.get_node_by_thing(file_thing).await {
                     if let Ok(Some(file_rec)) = self.store.get_file(&file_node.file_path).await {
                         let core_file = emry_core::models::File {
                             id: file_rec.id.as_ref().map(|t| t.to_string()).unwrap_or_default(),
                             path: file_rec.path.clone(),
                             language: emry_core::models::Language::from_name(&file_rec.language),
                             content: file_rec.content.clone(),
                         };
                         related_files.push(core_file);
                     }
                }

                if let Ok(in_edges) = self.store.get_neighbors(&anchor_id_str, "in").await {
                    for edge in in_edges {
                        if edge.relation == "contains" {
                            // The source is the Symbol
                            let symbol_id = edge.source.to_string();
                            if let Ok(Some(symbol_node)) = self.store.get_node(&symbol_id).await {
                                let sym = emry_core::models::Symbol {
                                    id: symbol_node.id.to_string(),
                                    name: symbol_node.label,
                                    kind: symbol_node.kind,
                                    file_path: std::path::PathBuf::from(&symbol_node.file_path),
                                    start_line: 0,
                                    end_line: 0,
                                    fqn: "".to_string(),
                                    language: emry_core::models::Language::Unknown,
                                    doc_comment: None,
                                    parent_scope: None,
                                };
                                related_symbols.push(sym);
                                edges.push((symbol_id.clone(), anchor_id_str.clone(), "contains".to_string()));
                                
                                // Fetch sibling chunks
                                if let Ok(parent_edges) = self.store.get_neighbors(&anchor_id_str, "in").await {
                                    for parent_edge in parent_edges {
                                        if parent_edge.relation == "contains" {
                                            let symbol_id = parent_edge.source.to_string();
                                            // Fetch Symbol Node
                                            if let Ok(Some(_)) = self.store.get_node(&symbol_id).await {
                                                // Now fetch all chunks contained by this parent symbol (siblings)
                                                if let Ok(child_edges) = self.store.get_neighbors(&symbol_id, "out").await {
                                                    for child_edge in child_edges {
                                                        if child_edge.relation == "contains" {
                                                            let child_chunk_id = child_edge.target.to_string();
                                                            // Don't add if it's the anchor itself (already in list)
                                                            if child_chunk_id != anchor_id_str {
                                                                if let Ok(Some(chunk_rec)) = self.store.get_chunk(&child_chunk_id).await {
                                                                    context_chunks.push(chunk_rec);
                                                                }
                                                            }
                                                            edges.push((symbol_id.clone(), child_chunk_id, "contains".to_string()));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }


                                // Now find what this Symbol calls/imports
                                if let Ok(out_edges) = self.store.get_neighbors(&symbol_id, "out").await {
                                    for out_edge in out_edges {
                                        let target_id = out_edge.target.to_string();
                                        if let Ok(Some(target_node)) = self.store.get_node(&target_id).await {
                                            let target_sym = emry_core::models::Symbol {
                                                id: target_node.id.to_string(),
                                                name: target_node.label,
                                                kind: target_node.kind,
                                                file_path: std::path::PathBuf::from(&target_node.file_path),
                                                start_line: 0,
                                                end_line: 0,
                                                fqn: "".to_string(),
                                                language: emry_core::models::Language::Unknown,
                                                doc_comment: None,
                                                parent_scope: None,
                                            };
                                            related_symbols.push(target_sym);
                                            edges.push((symbol_id.clone(), target_id, out_edge.relation));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        context_chunks.sort_by(|a, b| a.id.cmp(&b.id));
        context_chunks.dedup_by(|a, b| a.id == b.id);
        
        let anchor_ids: std::collections::HashSet<String> = anchors.iter().filter_map(|c| c.id.as_ref().map(|t| t.to_string())).collect();
        for chunk in context_chunks {
            if let Some(id) = &chunk.id {
                if !anchor_ids.contains(&id.to_string()) {
                    anchors.push(chunk);
                }
            }
        }

        let file_map: std::collections::HashMap<String, std::path::PathBuf> = related_files.iter()
            .map(|f| (f.id.clone(), std::path::PathBuf::from(&f.path)))
            .collect();
        
        let final_anchors: Vec<emry_core::models::ScoredChunk> = anchors.iter().map(|c| {
            let file_id = c.file.id.to_string();
            let path = file_map.get(&file_id).cloned().unwrap_or_else(|| std::path::PathBuf::from(&file_id));
            
            let core_chunk = emry_core::models::Chunk {
                id: c.id.as_ref().map(|t| t.to_string()).unwrap_or_default(),
                language: emry_core::models::Language::Unknown, 
                file_path: path,
                start_line: c.start_line,
                end_line: c.end_line,
                start_byte: None,
                end_byte: None,
                node_type: "chunk".to_string(),
                content_hash: "".to_string(),
                content: c.content.clone(),
                embedding: c.embedding.clone(),
                parent_scope: None,
                scope_path: c.scopes.clone(),
            };
            
            emry_core::models::ScoredChunk {
                score: if anchor_ids.contains(&core_chunk.id) { 1.0 } else { 0.5 },
                lexical_score: None,
                vector_score: None,
                graph_boost: None,
                graph_distance: None,
                graph_path: None,
                symbol_boost: None,
                chunk: core_chunk,
            }
        }).collect();

        Ok(emry_core::models::ContextGraph {
            anchors: final_anchors,
            related_files,
            related_symbols,
            edges,
        })
    }
}
