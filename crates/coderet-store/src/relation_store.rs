use crate::storage::{Store, Tree};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelationType {
    Defines,
    Calls,
    Imports,
}

pub struct RelationStore {
    relations_tree: Tree, // key: chunk_id:target_id, value: type
}

impl RelationStore {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self {
            relations_tree: store.open_tree("relations")?,
        })
    }

    pub fn add_relation(
        &self,
        source_id: &str,
        target_id: &str,
        rel_type: RelationType,
    ) -> Result<()> {
        let key = format!("{}:{}", source_id, target_id);
        self.relations_tree.insert_encoded(key.as_bytes(), &rel_type)?;
        Ok(())
    }

    pub fn delete_by_source(&self, source_id: &str) -> Result<()> {
        let prefix = format!("{}:", source_id);
        let mut keys_to_remove = Vec::new();
        for item in self.relations_tree.scan_prefix(prefix.as_bytes()) {
            let (k, _) = item?;
            keys_to_remove.push(k);
        }
        for k in keys_to_remove {
            self.relations_tree.remove(k)?;
        }
        Ok(())
    }

    pub fn delete_by_target(&self, target_id: &str) -> Result<()> {
        let suffix = format!(":{}", target_id);
        let mut keys_to_remove = Vec::new();

        for item in self.relations_tree.iter() {
            let (k, _) = item?;
            if let Ok(key_str) = std::str::from_utf8(&k) {
                if key_str.ends_with(&suffix) {
                    keys_to_remove.push(k.clone());
                }
            }
        }

        for k in keys_to_remove {
            self.relations_tree.remove(k)?;
        }
        Ok(())
    }

    pub fn get_relations_from(&self, source_id: &str) -> Result<Vec<(String, RelationType)>> {
        let mut out = Vec::new();
        let prefix = format!("{}:", source_id);
        for item in self.relations_tree.scan_prefix(prefix.as_bytes()) {
            let (k, v) = item?;
            if let Ok(key_str) = std::str::from_utf8(&k) {
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
            if let Ok(key_str) = std::str::from_utf8(&k) {
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