pub mod fs;
pub mod graph;
pub mod search;
pub mod summaries;

use async_trait::async_trait;
use std::sync::Arc;

use coderet_context::RepoContext;
use coderet_pipeline::manager::IndexManager;

use self::{fs::FsTool, graph::GraphTool, search::Search, summaries::SummaryTool};



#[async_trait(?Send)]
pub trait SummaryToolTrait: Send + Sync {
    async fn search_summaries(
        &self,
        query: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<coderet_context::types::SummaryHit>>;
    async fn repo_and_module_summaries(
        &self,
        top_k: usize,
    ) -> anyhow::Result<Vec<coderet_context::types::SummaryHit>>;
}

use self::fs::DirEntry;
use self::graph::{GraphDirection, GraphResult};
use std::path::Path;

pub trait GraphToolTrait: Send + Sync {
    fn graph(&self, symbol: &str, direction: GraphDirection, max_hops: usize) -> anyhow::Result<GraphResult>;
}

pub trait FsToolTrait: Send + Sync {
    fn list_files(&self, dir_path: &Path, limit: Option<usize>) -> anyhow::Result<Vec<DirEntry>>;
    fn outline(&self, path: &Path) -> anyhow::Result<Vec<coderet_core::models::Symbol>>;
    fn read_file_span(
        &self,
        path: &std::path::Path,
        start: usize,
        end: usize,
    ) -> anyhow::Result<String>;
}

/// Convenience bundle of all agent tools.
pub struct AgentTools {
    pub search: Search,
    pub summaries: SummaryTool,
    pub graph: GraphTool,
    pub fs: FsTool,
}

impl AgentTools {
    pub fn new(ctx: Arc<RepoContext>, manager: Arc<IndexManager>) -> Self {
        Self {
            search: Search::new(ctx.clone(), manager),
            summaries: SummaryTool::new(ctx.clone()),
            graph: GraphTool::new(ctx.clone()),
            fs: FsTool::new(ctx),
        }
    }
}