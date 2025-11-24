use crate::context::RepoContext;
use crate::types::SummaryHit;
use anyhow::Result;
use std::sync::Arc;

use super::SummaryToolTrait;

pub struct SummaryTool {
    ctx: Arc<RepoContext>,
}

impl SummaryTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    pub async fn search_summaries(&self, query: &str, top_k: usize) -> Result<Vec<SummaryHit>> {
        let mut out = Vec::new();
        let guard = self.ctx.summary_index.lock().await;
        let embedder = self
            .ctx
            .embedder
            .as_ref()
            .map(|e| e.as_ref() as &dyn coderet_core::traits::Embedder);
        if let Ok(results) = guard.search_structured(query, top_k, embedder).await {
            for (score, summary) in results {
                out.push(SummaryHit { score, summary });
            }
        };
        Ok(out)
    }

    /// Fetch repo/module summaries for planner context.
    pub async fn repo_and_module_summaries(&self, top_k: usize) -> Result<Vec<SummaryHit>> {
        let mut out = Vec::new();
        let guard = self.ctx.summary_index.lock().await;
        if let Ok(results) = guard.get_repo_and_module_summaries(top_k).await {
            for summary in results {
                out.push(SummaryHit {
                    score: 0.0,
                    summary,
                });
            }
        }
        Ok(out)
    }
}

#[async_trait::async_trait(?Send)]
impl SummaryToolTrait for SummaryTool {
    async fn search_summaries(&self, query: &str, top_k: usize) -> Result<Vec<SummaryHit>> {
        SummaryTool::search_summaries(self, query, top_k).await
    }

    async fn repo_and_module_summaries(&self, top_k: usize) -> Result<Vec<SummaryHit>> {
        SummaryTool::repo_and_module_summaries(self, top_k).await
    }
}
