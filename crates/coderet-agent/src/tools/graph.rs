use crate::context::RepoContext;
use crate::types::{GraphEdge, GraphSubgraph};
use anyhow::Result;
use coderet_store::relation_store::RelationType;
use std::sync::Arc;

use super::GraphToolTrait;

pub struct GraphTool {
    ctx: Arc<RepoContext>,
}

impl GraphTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    /// Collect a small subgraph around `node_id`, following outgoing edges up to `max_hops`.
    /// If `kinds` is non-empty, only edges matching those kinds are included.
    pub fn neighbors(
        &self,
        node_id: &str,
        kinds: &[String],
        max_hops: usize,
    ) -> Result<GraphSubgraph> {
        let rels: Vec<RelationType> = kinds
            .iter()
            .map(|k| match k.as_str() {
                "calls" => RelationType::Calls,
                "imports" => RelationType::Imports,
                "defines" => RelationType::Defines,
                _ => RelationType::Calls,
            })
            .collect();
        let sub = self.ctx.graph.neighbors(node_id, &rels, max_hops as u8)?;
        Ok(GraphSubgraph {
            nodes: sub
                .nodes
                .into_iter()
                .map(|n| coderet_graph::graph::GraphNode {
                    id: n.id,
                    kind: n.kind,
                    label: n.label,
                    canonical_id: None,
                    file_path: n.file_path.unwrap_or_default(),
                })
                .collect(),
            edges: sub
                .edges
                .into_iter()
                .map(|e| GraphEdge {
                    source: e.src,
                    target: e.dst,
                    kind: e.relation,
                })
                .collect(),
        })
    }

    /// Find shortest paths (by edge count) between two nodes, limited by max_hops.
    pub fn shortest_paths(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
    ) -> Result<Vec<Vec<String>>> {
        self.ctx.graph.shortest_paths(from, to, &[], max_hops)
    }

    /// Relation-filtered shortest paths.
    pub fn shortest_paths_with_kinds(
        &self,
        from: &str,
        to: &str,
        kinds: &[String],
        max_hops: usize,
    ) -> Result<Vec<Vec<String>>> {
        let rels: Vec<RelationType> = kinds
            .iter()
            .map(|k| match k.as_str() {
                "calls" => RelationType::Calls,
                "imports" => RelationType::Imports,
                "defines" => RelationType::Defines,
                _ => RelationType::Calls,
            })
            .collect();
        self.ctx.graph.shortest_paths(from, to, &rels, max_hops)
    }
}

impl GraphToolTrait for GraphTool {
    fn neighbors(&self, node_id: &str, kinds: &[String], max_hops: usize) -> Result<GraphSubgraph> {
        GraphTool::neighbors(self, node_id, kinds, max_hops)
    }

    fn shortest_paths(&self, from: &str, to: &str, max_hops: usize) -> Result<Vec<Vec<String>>> {
        GraphTool::shortest_paths(self, from, to, max_hops)
    }
}
