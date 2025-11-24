pub mod fs;
pub mod graph;
pub mod search;
pub mod summaries;

use async_trait::async_trait;
use std::sync::Arc;

use crate::context::RepoContext;

use self::{fs::FsTool, graph::GraphTool, search::SearchTool, summaries::SummaryTool};

/// Tool traits matching the spec for planner/executor wiring.
#[async_trait(?Send)]
pub trait SearchToolTrait: Send + Sync {
    async fn search_chunks(
        &self,
        query: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<crate::types::ChunkHit>>;
    async fn search_chunks_with_keywords(
        &self,
        query: &str,
        keywords: &[String],
        top_k: usize,
    ) -> anyhow::Result<Vec<crate::types::ChunkHit>>;
    fn search_symbols(&self, name: &str) -> anyhow::Result<Vec<crate::types::SymbolHit>>;
    fn list_entry_points(&self) -> anyhow::Result<Vec<crate::types::SymbolHit>>;
}

#[async_trait(?Send)]
pub trait SummaryToolTrait: Send + Sync {
    async fn search_summaries(
        &self,
        query: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<crate::types::SummaryHit>>;
    async fn repo_and_module_summaries(
        &self,
        top_k: usize,
    ) -> anyhow::Result<Vec<crate::types::SummaryHit>>;
}

pub trait GraphToolTrait: Send + Sync {
    fn neighbors(
        &self,
        node_id: &str,
        kinds: &[String],
        max_hops: usize,
    ) -> anyhow::Result<crate::types::GraphSubgraph>;
    fn shortest_paths(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
    ) -> anyhow::Result<Vec<Vec<String>>>;
}

pub trait FsToolTrait: Send + Sync {
    fn list_files(&self, limit: Option<usize>) -> anyhow::Result<Vec<std::path::PathBuf>>;
    fn list_files_matching(
        &self,
        pattern: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<std::path::PathBuf>>;
    fn read_file_span(
        &self,
        path: &std::path::Path,
        start: usize,
        end: usize,
    ) -> anyhow::Result<String>;
}

/// Convenience bundle of all agent tools.
pub struct AgentTools {
    pub search: SearchTool,
    pub summaries: SummaryTool,
    pub graph: GraphTool,
    pub fs: FsTool,
}

impl AgentTools {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self {
            search: SearchTool::new(ctx.clone()),
            summaries: SummaryTool::new(ctx.clone()),
            graph: GraphTool::new(ctx.clone()),
            fs: FsTool::new(ctx),
        }
    }
}
