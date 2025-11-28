pub mod fs;
pub mod graph;
pub mod search;



use std::sync::Arc;

use coderet_context::RepoContext;
use coderet_pipeline::manager::IndexManager;

use self::{fs::FsTool, graph::GraphTool, search::Search};



use self::fs::DirEntry;
use self::graph::{GraphDirection, GraphResult};
use std::path::Path;

pub trait GraphToolTrait: Send + Sync {
    fn graph(&self, symbol: &str, direction: GraphDirection, max_hops: usize) -> anyhow::Result<GraphResult>;
}

pub trait FsToolTrait: Send + Sync {
    fn list_files(&self, dir_path: &Path, depth: usize, limit: Option<usize>) -> anyhow::Result<Vec<DirEntry>>;
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
    pub graph: GraphTool,
    pub fs: FsTool,
}

impl AgentTools {
    pub fn new(ctx: Arc<RepoContext>, manager: Arc<IndexManager>) -> Self {
        Self {
            search: Search::new(ctx.clone(), manager),
            graph: GraphTool::new(ctx.clone()),
            fs: FsTool::new(ctx),
        }
    }
}