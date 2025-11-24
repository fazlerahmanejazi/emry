use anyhow::Result;
use sled::Db;

/// Content-addressable storage for chunk bodies. Keeps metadata elsewhere (ChunkStore).
pub struct ContentStore {
    content_tree: sled::Tree,
}

impl ContentStore {
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self {
            content_tree: db.open_tree("chunk_content")?,
        })
    }

    /// Store content by hash; idempotent.
    pub fn put(&self, hash: &str, content: &str) -> Result<()> {
        if self.content_tree.contains_key(hash.as_bytes())? {
            return Ok(());
        }
        self.content_tree
            .insert(hash.as_bytes(), content.as_bytes())?;
        Ok(())
    }

    pub fn get(&self, hash: &str) -> Result<Option<String>> {
        if let Some(bytes) = self.content_tree.get(hash.as_bytes())? {
            let s = String::from_utf8(bytes.to_vec()).unwrap_or_default();
            Ok(Some(s))
        } else {
            Ok(None)
        }
    }
}
