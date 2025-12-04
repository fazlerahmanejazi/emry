use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use crate::ops::architecture::ArchitectureTool as InnerArchitectureTool;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::llm::OpenAIProvider;
use crate::workflows::architecture::ArchitectureWorkflow;
use crate::ops::fs::FsTool;
use emry_engine::search::service::SearchService;
use crate::project::context::RepoContext;
use crate::cortex::CortexEvent;

pub struct DescribeArchitectureTool {
    workflow: Arc<ArchitectureWorkflow>,
}

impl DescribeArchitectureTool {
    pub fn new(
        inner: Arc<InnerArchitectureTool>, 
        fs: Arc<FsTool>, 
        llm: OpenAIProvider,
        ctx: Arc<RepoContext>,
        search: Arc<SearchService>
    ) -> Self {
        let workflow = Arc::new(ArchitectureWorkflow::new(inner, fs, llm, ctx, search));
        Self { workflow }
    }

    pub async fn run_analysis<F>(&self, mode: &str, callback: F) -> Result<String> 
    where F: FnMut(CortexEvent) + Send + Sync + 'static
    {
        self.workflow.run_analysis(mode, callback).await
    }
}

#[async_trait]
impl Tool for DescribeArchitectureTool {
    fn name(&self) -> &str {
        "describe_architecture"
    }

    fn description(&self) -> &str {
        "Investigate and describe the architecture of the codebase. It analyzes module coupling, identifies central hubs, and samples key files to generate a comprehensive architectural report."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "description": "Analysis mode: 'fast' (default, uses graph centrality) or 'deep' (recursive directory analysis, more comprehensive but slower)."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let mode = args["mode"].as_str().unwrap_or("fast");
        self.run_analysis(mode, |_| {}).await
    }
}
