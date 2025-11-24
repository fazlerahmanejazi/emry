use anyhow::Result;
use coderet_core::models::Chunk;
use serde::{Deserialize, Serialize};
use sled::Db;

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
    chunks_tree: sled::Tree,
    file_chunks_tree: sled::Tree, // file_id -> Vec<chunk_id>
}

impl ChunkStore {
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self {
            chunks_tree: db.open_tree("chunks")?,
            file_chunks_tree: db.open_tree("file_chunks")?,
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

        let bytes = bincode::serialize(&stored)?;
        self.chunks_tree.insert(chunk.id.as_bytes(), bytes)?;

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
            let mut chunks: Vec<String> = bincode::deserialize(&val)?;
            let before = chunks.len();
            chunks.retain(|c| !to_remove.contains(c));
            if chunks.len() != before {
                if chunks.is_empty() {
                    let _ = self.file_chunks_tree.remove(key)?;
                } else {
                    let bytes = bincode::serialize(&chunks)?;
                    self.file_chunks_tree.insert(key, bytes)?;
                }
            }
        }

        Ok(())
    }

    fn add_chunk_to_file_index(&self, file_id: u64, chunk_id: &str) -> Result<()> {
        let key = file_id.to_be_bytes();
        let mut chunks: Vec<String> = if let Some(bytes) = self.file_chunks_tree.get(&key)? {
            bincode::deserialize(&bytes)?
        } else {
            Vec::new()
        };

        if !chunks.contains(&chunk_id.to_string()) {
            chunks.push(chunk_id.to_string());
            let bytes = bincode::serialize(&chunks)?;
            self.file_chunks_tree.insert(&key, bytes)?;
        }
        Ok(())
    }

    pub fn get_chunk(&self, id: &str) -> Result<Option<StoredChunk>> {
        if let Some(bytes) = self.chunks_tree.get(id.as_bytes())? {
            let chunk: StoredChunk = bincode::deserialize(&bytes)?;
            Ok(Some(chunk))
        } else {
            Ok(None)
        }
    }

    pub fn get_chunks_for_file(&self, file_id: u64) -> Result<Vec<String>> {
        if let Some(bytes) = self.file_chunks_tree.get(&file_id.to_be_bytes())? {
            let chunks: Vec<String> = bincode::deserialize(&bytes)?;
            Ok(chunks)
        } else {
            Ok(Vec::new())
        }
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
