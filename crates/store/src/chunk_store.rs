use crate::storage::{Store, Tree};
use anyhow::Result;
use emry_core::models::Chunk;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredChunk {
    pub id: String,
    pub file_id: u64,
    pub start_line: usize,
    pub end_line: usize,
    pub node_type: String,
    pub content_hash: String,
    // Content is NOT stored here to save space, retrieved from FileStore + lines
}

pub struct ChunkStore {
    chunks_tree: Tree,
    file_chunks_tree: Tree, // file_id -> Vec<chunk_id>
}

impl ChunkStore {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self {
            chunks_tree: store.open_tree("chunks")?,
            file_chunks_tree: store.open_tree("file_chunks")?,
        })
    }

    pub fn add_chunk(&self, chunk: &Chunk, file_id: u64) -> Result<()> {
        let stored = StoredChunk {
            id: chunk.id.clone(),
            file_id,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            node_type: chunk.node_type.clone(),
            content_hash: chunk.content_hash.clone(),
        };

        self.chunks_tree.insert_encoded(chunk.id.as_bytes(), &stored)?;

        // Update file_chunks index
        self.add_chunk_to_file_index(file_id, &chunk.id)?;

        Ok(())
    }

    /// Remove specific chunks (by id) and keep file index in sync.
    pub fn delete_chunks(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        // Build a set for fast lookup
        let to_remove: std::collections::HashSet<String> = ids.iter().cloned().collect();

        // Remove from main chunk tree
        for id in &to_remove {
            let _ = self.chunks_tree.remove(id.as_bytes())?;
        }

        // Update file->chunks index
        for item in self.file_chunks_tree.iter() {
            let (key, val) = item?;
            let mut chunks: Vec<String> = self.file_chunks_tree.get_decoded(&val)?.unwrap_or_default();
            let before = chunks.len();
            chunks.retain(|c| !to_remove.contains(c));
            if chunks.len() != before {
                if chunks.is_empty() {
                    let _ = self.file_chunks_tree.remove(key)?;
                } else {
                    self.file_chunks_tree.insert_encoded(key, &chunks)?;
                }
            }
        }

        Ok(())
    }

    fn add_chunk_to_file_index(&self, file_id: u64, chunk_id: &str) -> Result<()> {
        let key = file_id.to_be_bytes();
        let mut chunks: Vec<String> = self.file_chunks_tree.get_decoded(&key)?.unwrap_or_default();

        if !chunks.contains(&chunk_id.to_string()) {
            chunks.push(chunk_id.to_string());
            self.file_chunks_tree.insert_encoded(&key, &chunks)?;
        }
        Ok(())
    }

    pub fn get_chunk(&self, id: &str) -> Result<Option<StoredChunk>> {
        self.chunks_tree.get_decoded(id.as_bytes())
    }

    pub fn get_chunks_for_file(&self, file_id: u64) -> Result<Vec<String>> {
        Ok(self.file_chunks_tree.get_decoded(&file_id.to_be_bytes())?.unwrap_or_default())
    }

    pub fn delete_chunks_for_file(&self, file_id: u64) -> Result<()> {
        let chunk_ids = self.get_chunks_for_file(file_id)?;
        for id in chunk_ids {
            self.chunks_tree.remove(id.as_bytes())?;
        }
        self.file_chunks_tree.remove(&file_id.to_be_bytes())?;
        Ok(())
    }
}