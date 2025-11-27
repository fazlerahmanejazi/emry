use crate::storage::{Store, Tree};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitEntry {
    pub id: String,
    pub timestamp: u64,
    pub note: String,
}

pub struct CommitLog {
    log: Tree,
}

impl CommitLog {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self {
            log: store.open_tree("commit_log")?,
        })
    }

    pub fn append(&self, entry: CommitEntry) -> Result<()> {
        let bytes = bincode::serialize(&entry)?;
        self.log.insert(entry.id.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<CommitEntry>> {
        let mut out = Vec::new();
        for item in self.log.iter().rev().take(limit) {
            let (_, v) = item?;
            if let Ok(entry) = bincode::deserialize::<CommitEntry>(&v) {
                out.push(entry);
            }
        }
        Ok(out)
    }
}