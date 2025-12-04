use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::project::context::RepoContext;
use crate::llm::OpenAIProvider;
use emry_engine::search::service::SearchService;
use crate::ops::fs::FsTool;
use crate::ops::graph::GraphTool;
use crate::cortex::CortexEvent;

use crate::workflows::impact::ImpactWorkflow;

pub struct AnalyzeImpactTool {
    workflow: Arc<ImpactWorkflow>,
}

impl AnalyzeImpactTool {
    pub fn new(
        ctx: Arc<RepoContext>,
        llm: OpenAIProvider,
        fs: Arc<FsTool>,
        graph: Arc<GraphTool>,
        search: Arc<SearchService>,
    ) -> Result<Self> {
        let workflow = Arc::new(ImpactWorkflow::new(ctx, llm, fs, graph, search)?);
        Ok(Self { workflow })
    }

    pub async fn run_analysis<F>(&self, file_path: &str, start_line: usize, end_line: usize, callback: F) -> Result<String> 
    where F: FnMut(CortexEvent) + Send + Sync + 'static
    {
        self.workflow.run_analysis(file_path, start_line, end_line, callback).await
    }
}

#[async_trait]
impl Tool for AnalyzeImpactTool {
    fn name(&self) -> &str {
        "analyze_impact"
    }

    fn description(&self) -> &str {
        "Analyze the impact of changes in a specific file. Identifies which symbols are modified and what other parts of the codebase depend on them (risk analysis)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The path of the file that was changed."
                },
                "start_line": {
                    "type": "integer",
                    "description": "The starting line number of the change."
                },
                "end_line": {
                    "type": "integer",
                    "description": "The ending line number of the change."
                }
            },
            "required": ["file_path", "start_line", "end_line"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let file_path = args["file_path"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path' argument"))?;
        let start_line = args["start_line"].as_u64().unwrap_or(0) as usize;
        let end_line = args["end_line"].as_u64().unwrap_or(0) as usize;

        self.run_analysis(file_path, start_line, end_line, |_| {}).await
    }
}
