use crate::structure::graph::{GraphNode, NodeId};
use anyhow::Result;
use std::path::Path;

use std::fmt::Debug;

/// Trait for persistent node storage.
/// Separation of concerns: Graph structure (edges) stays in RAM (Petgraph),
/// heavy node data (content, metadata) goes to Disk (Sled).
pub trait NodeStorage: Send + Sync + Debug {
    fn get(&self, id: &NodeId) -> Result<Option<GraphNode>>;
    fn insert(&self, id: &NodeId, node: &GraphNode) -> Result<()>;
    fn contains(&self, id: &NodeId) -> Result<bool>;
    fn flush(&self) -> Result<()>;
}

pub struct SledStorage {
    db: sled::Db,
}

impl Debug for SledStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SledStorage")
            .field("db", &"sled::Db")
            .finish()
    }
}

impl SledStorage {
    pub fn new(path: &Path) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }
}

impl NodeStorage for SledStorage {
    fn get(&self, id: &NodeId) -> Result<Option<GraphNode>> {
        let key = id.0.as_bytes();
        match self.db.get(key)? {
            Some(ivec) => {
                let node: GraphNode = bincode::deserialize(&ivec)?;
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    fn insert(&self, id: &NodeId, node: &GraphNode) -> Result<()> {
        let key = id.0.as_bytes();
        let value = bincode::serialize(node)?;
        self.db.insert(key, value)?;
        Ok(())
    }

    fn contains(&self, id: &NodeId) -> Result<bool> {
        Ok(self.db.contains_key(id.0.as_bytes())?)
    }

    fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::graph::{NodeType, CodeGraph};
    use tempfile::tempdir;
    use std::sync::Arc;

    #[test]
    fn test_sled_storage_persistence() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("db");
        let storage = Arc::new(SledStorage::new(&db_path)?);

        let id = NodeId("test_node".to_string());
        let node = GraphNode {
            id: id.clone(),
            kind: NodeType::Function,
            label: "test".to_string(),
            fqn: None,
            language: None,
            file_path: None,
            start_line: 0,
            end_line: 0,
            chunk_ids: vec![],
        };

        storage.insert(&id, &node)?;
        
        // Verify we can read it back
        let loaded = storage.get(&id)?.expect("Node should exist");
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.label, "test");

        Ok(())
    }

    #[test]
    fn test_codegraph_integration() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("db");
        let storage = Arc::new(SledStorage::new(&db_path)?);
        
        let graph_path = dir.path().join("graph.json");
        let mut graph = CodeGraph::new(&graph_path).with_storage(storage);

        let id = NodeId("integrated_node".to_string());
        graph.add_node(
            id.clone(),
            NodeType::Class,
            "Integrated".to_string(),
            None,
            None,
            None,
            1,
            10,
            vec![],
        );

        // Check it's in the graph (via get_node which checks storage)
        let node = graph.get_node(&id).expect("Node should be retrievable");
        assert_eq!(node.label, "Integrated");

        Ok(())
    }
}
