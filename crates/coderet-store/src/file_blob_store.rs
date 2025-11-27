use crate::storage::{Store, Tree};
use anyhow::Result;
use sha2::{Digest, Sha256};

/// Content-addressable storage for full file blobs to support dedup/versioning.
pub struct FileBlobStore {
    blobs: Tree,
    path_to_hash: Tree,
}

impl FileBlobStore {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self {
            blobs: store.open_tree("file_blobs")?,
            path_to_hash: store.open_tree("file_path_hash")?,
        })
    }

    pub fn put(&self, path: &std::path::Path, content: &str) -> Result<String> {
        let hash = hash_str(content);
        if !self.blobs.contains_key(hash.as_bytes())? {
            self.blobs.insert(hash.as_bytes(), content.as_bytes())?;
        }
        let path_str = path.to_string_lossy();
        self.path_to_hash
            .insert(path_str.as_bytes(), hash.as_bytes())?;
        Ok(hash)
    }

    pub fn get_by_hash(&self, hash: &str) -> Result<Option<String>> {
        if let Some(bytes) = self.blobs.get(hash.as_bytes())? {
            let s = String::from_utf8(bytes).unwrap_or_default();
            return Ok(Some(s));
        }
        Ok(None)
    }

    pub fn get_for_path(&self, path: &std::path::Path) -> Result<Option<String>> {
        let path_str = path.to_string_lossy();
        if let Some(hash) = self
            .path_to_hash
            .get(path_str.as_bytes())?
            .and_then(|b| String::from_utf8(b).ok())
        {
            return self.get_by_hash(&hash);
        }
        Ok(None)
    }
}

fn hash_str(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}