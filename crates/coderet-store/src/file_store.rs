use crate::storage::{Store, Tree};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub id: u64,
    pub path: PathBuf,
    pub content_hash: String,
    pub last_modified: u64,
    pub last_indexed_run: Option<u64>,
}

pub struct FileStore {
    files_tree: Tree,
    path_to_id_tree: Tree,
    hash_to_id_tree: Tree,
    next_id: AtomicU64,
}

impl FileStore {
    pub fn new(store: Store) -> Result<Self> {
        let files_tree = store.open_tree("files")?;
        let path_to_id_tree = store.open_tree("path_to_id")?;
        let hash_to_id_tree = store.open_tree("hash_to_id")?;

        // Initialize next_id
        let last_id = files_tree
            .last()?
            .map(|(k, _)| u64::from_be_bytes(k.as_slice().try_into().unwrap()))
            .unwrap_or(0);

        Ok(Self {
            files_tree,
            path_to_id_tree,
            hash_to_id_tree,
            next_id: AtomicU64::new(last_id + 1),
        })
    }

    pub fn get_or_create_file_id(&self, path: &Path, content_hash: &str) -> Result<u64> {
        // Prefer hash-based ID to allow dedup across moves.
        if let Some(id_bytes) = self.hash_to_id_tree.get(content_hash.as_bytes())? {
            let id = u64::from_be_bytes(id_bytes.as_slice().try_into()?);
            self.path_to_id_tree
                .insert(path.to_string_lossy().as_bytes(), &id.to_be_bytes())?;
            return Ok(id);
        }
        let path_str = path.to_string_lossy();
        if let Some(id_bytes) = self.path_to_id_tree.get(path_str.as_bytes())? {
            return Ok(u64::from_be_bytes(id_bytes.as_slice().try_into()?));
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.path_to_id_tree
            .insert(path_str.as_bytes(), &id.to_be_bytes())?;
        self.hash_to_id_tree
            .insert(content_hash.as_bytes(), &id.to_be_bytes())?;
        Ok(id)
    }

    pub fn update_file_metadata(&self, metadata: FileMetadata) -> Result<()> {
        let id = metadata.id;
        let bytes = bincode::serialize(&metadata)?;
        self.files_tree.insert(&id.to_be_bytes(), bytes)?;
        self.hash_to_id_tree
            .insert(metadata.content_hash.as_bytes(), &id.to_be_bytes())?;
        Ok(())
    }

    pub fn get_or_create_file_id_by_path_str(&self, path: &str) -> Result<u64> {
        // Simple implementation: check if exists, else create new ID
        for item in self.files_tree.iter() {
            let (key_bytes, val_bytes) = item?;
            if let Ok(meta) = bincode::deserialize::<FileMetadata>(&val_bytes) {
                if meta.path.to_string_lossy() == path {
                    return Ok(u64::from_be_bytes(key_bytes.as_slice().try_into()?));
                }
            }
        }
        // Not found, create new
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(id)
    }

    pub fn get_file_metadata(&self, id: u64) -> Result<Option<FileMetadata>> {
        if let Some(bytes) = self.files_tree.get(&id.to_be_bytes())? {
            let meta: FileMetadata = bincode::deserialize(&bytes)?;
            Ok(Some(meta))
        } else {
            Ok(None)
        }
    }

    pub fn get_file_id(&self, path: &Path) -> Result<Option<u64>> {
        let path_str = path.to_string_lossy();
        if let Some(id_bytes) = self.path_to_id_tree.get(path_str.as_bytes())? {
            Ok(Some(u64::from_be_bytes(id_bytes.as_slice().try_into()?)))
        } else {
            Ok(None)
        }
    }

    pub fn list_metadata(&self) -> Result<Vec<FileMetadata>> {
        let mut files = Vec::new();
        for item in self.files_tree.iter() {
            let (_, val_bytes) = item?;
            if let Ok(meta) = bincode::deserialize::<FileMetadata>(&val_bytes) {
                files.push(meta);
            }
        }
        Ok(files)
    }

    pub fn delete_file(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy();
        if let Some(id_bytes) = self.path_to_id_tree.remove(path_str.as_bytes())? {
            let id = u64::from_be_bytes(id_bytes.as_slice().try_into()?);
            let _ = self.files_tree.remove(&id.to_be_bytes())?;
        }
        Ok(())
    }

    pub fn get_id_by_hash(&self, hash: &str) -> Result<Option<u64>> {
        if let Some(id_bytes) = self.hash_to_id_tree.get(hash.as_bytes())? {
            return Ok(Some(u64::from_be_bytes(id_bytes.as_slice().try_into()?)));
        }
        Ok(None)
    }
}