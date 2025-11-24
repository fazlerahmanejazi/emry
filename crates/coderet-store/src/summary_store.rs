use anyhow::Result;
use coderet_core::models::Summary;
use sled::Db;

/// Sled-backed storage for summaries keyed by target + level to avoid duplicates.
pub struct SummaryStore {
    tree: sled::Tree,
}

impl SummaryStore {
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self {
            tree: db.open_tree("summaries")?,
        })
    }

    fn key_for(summary: &Summary) -> String {
        format!("{}|{:?}", summary.target_id, summary.level)
    }

    pub fn put(&self, summary: &Summary) -> Result<()> {
        let key = Self::key_for(summary);
        let bytes = bincode::serialize(summary)?;
        self.tree.insert(key.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn put_many(&self, summaries: &[Summary]) -> Result<()> {
        let mut batch = sled::Batch::default();
        for s in summaries {
            let key = Self::key_for(s);
            let bytes = bincode::serialize(s)?;
            batch.insert(key.as_bytes(), bytes);
        }
        self.tree.apply_batch(batch)?;
        Ok(())
    }

    pub fn remove_targets(&self, targets: &[String]) -> Result<()> {
        if targets.is_empty() {
            return Ok(());
        }
        let target_set: std::collections::HashSet<String> = targets.iter().cloned().collect();
        let mut batch = sled::Batch::default();
        for item in self.tree.iter() {
            let (key, val) = item?;
            if let Ok(summary) = bincode::deserialize::<Summary>(&val) {
                if target_set.contains(&summary.target_id) {
                    batch.remove(key);
                }
            }
        }
        self.tree.apply_batch(batch)?;
        Ok(())
    }

    pub fn remove_by_files(&self, files: &[std::path::PathBuf]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let set: std::collections::HashSet<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let mut batch = sled::Batch::default();
        for item in self.tree.iter() {
            let (key, val) = item?;
            if let Ok(summary) = bincode::deserialize::<Summary>(&val) {
                if let Some(fp) = &summary.file_path {
                    if set.contains(&fp.to_string_lossy().to_string()) {
                        batch.remove(key);
                    }
                }
            }
        }
        self.tree.apply_batch(batch)?;
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<Summary>> {
        let mut summaries = Vec::new();
        for item in self.tree.iter() {
            let (_, val) = item?;
            if let Ok(summary) = bincode::deserialize::<Summary>(&val) {
                summaries.push(summary);
            }
        }
        Ok(summaries)
    }
}
