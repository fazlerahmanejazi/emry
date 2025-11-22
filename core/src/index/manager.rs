use crate::models::{FileMetadata, IndexMetadata};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct IndexManager;

impl IndexManager {
    pub fn load(metadata_path: &Path) -> IndexMetadata {
        if metadata_path.exists() {
            std::fs::read_to_string(metadata_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            IndexMetadata::default()
        }
    }

    pub fn save(metadata_path: &Path, metadata: &IndexMetadata) -> Result<()> {
        if let Some(parent) = metadata_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(metadata)?;
        std::fs::write(metadata_path, content)?;
        Ok(())
    }

    pub fn compute_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn update_file_entry(
        metadata: &mut IndexMetadata,
        path: &Path,
        content_hash: String,
        chunk_ids: Vec<String>,
    ) {
        let mut updated = false;
        for entry in metadata.files.iter_mut() {
            if entry.path == path {
                entry.content_hash = content_hash.clone();
                entry.chunk_ids = chunk_ids.clone();
                entry.last_modified = current_ts();
                updated = true;
                break;
            }
        }
        if !updated {
            metadata.files.push(FileMetadata {
                path: path.to_path_buf(),
                content_hash,
                last_modified: current_ts(),
                chunk_ids,
            });
        }
    }
}

fn current_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
