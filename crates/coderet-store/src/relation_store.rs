use anyhow::Result;
use serde::{Deserialize, Serialize};
use sled::Db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelationType {
    Defines,
    Calls,
    Imports,
}

pub struct RelationStore {
    relations_tree: sled::Tree, // key: chunk_id:target_id, value: type
}

impl RelationStore {
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self {
            relations_tree: db.open_tree("relations")?,
        })
    }

    pub fn add_relation(
        &self,
        source_id: &str,
        target_id: &str,
        rel_type: RelationType,
    ) -> Result<()> {
        let key = format!("{}:{}", source_id, target_id);
        let bytes = bincode::serialize(&rel_type)?;
        self.relations_tree.insert(key.as_bytes(), bytes)?;
        Ok(())
    }

    pub fn delete_by_source(&self, source_id: &str) -> Result<()> {
        let prefix = format!("{}:", source_id);
        let mut batch = sled::Batch::default();
        for item in self.relations_tree.scan_prefix(prefix.as_bytes()) {
            let (k, _) = item?;
            batch.remove(k);
        }
        self.relations_tree.apply_batch(batch)?;
        Ok(())
    }

    pub fn delete_by_target(&self, target_id: &str) -> Result<()> {
        let suffix = format!(":{}", target_id);
        let mut batch = sled::Batch::default();

        for item in self.relations_tree.iter() {
            let (k, _) = item?;
            if let Ok(key_str) = std::str::from_utf8(k.as_ref()) {
                if key_str.ends_with(&suffix) {
                    batch.remove(k);
                }
            }
        }

        self.relations_tree.apply_batch(batch)?;
        Ok(())
    }

    pub fn get_relations_from(&self, source_id: &str) -> Result<Vec<(String, RelationType)>> {
        let mut out = Vec::new();
        let prefix = format!("{}:", source_id);
        for item in self.relations_tree.scan_prefix(prefix.as_bytes()) {
            let (k, v) = item?;
            if let Ok(key_str) = std::str::from_utf8(k.as_ref()) {
                if let Some((_, target)) = key_str.split_once(':') {
                    let rel: RelationType = bincode::deserialize(&v)?;
                    out.push((target.to_string(), rel));
                }
            }
        }
        Ok(out)
    }

    pub fn get_sources_for_target(&self, target_id: &str) -> Result<Vec<(String, RelationType)>> {
        let mut out = Vec::new();
        let suffix = format!(":{}", target_id);
        for item in self.relations_tree.iter() {
            let (k, v) = item?;
            if let Ok(key_str) = std::str::from_utf8(k.as_ref()) {
                if key_str.ends_with(&suffix) {
                    if let Some((source, _)) = key_str.split_once(':') {
                        let rel: RelationType = bincode::deserialize(&v)?;
                        out.push((source.to_string(), rel));
                    }
                }
            }
        }
        Ok(out)
    }
}
