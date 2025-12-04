use crate::project::context::RepoContext;
use anyhow::Result;
use emry_core::diff::{DiffAnalyzer, FileDiff};
use std::path::PathBuf;
use std::sync::Arc;
use crate::llm::OpenAIProvider;
use crate::cortex::{Cortex, CortexEvent};
use crate::cortex::context::AgentContext;
use emry_engine::search::service::SearchService;

use crate::ops::fs::FsTool;
use crate::ops::graph::GraphTool;
use crate::ops::search::Search;

use crate::cortex::tools::{
    fs::{ViewCodeItemTool, ViewFileOutlineTool},
    workflows::{ReadFilesTool, FindUsagesTool},
    graph::InspectGraphTool,
    search::SearchCodeTool,
};

pub struct ImpactWorkflow {
    ctx: Arc<RepoContext>,
    llm: OpenAIProvider,
    fs: Arc<FsTool>,
    graph: Arc<GraphTool>,
    search: Arc<SearchService>,
}

impl ImpactWorkflow {
    pub fn new(
        ctx: Arc<RepoContext>, 
        llm: OpenAIProvider,
        fs: Arc<FsTool>,
        graph: Arc<GraphTool>,
        search: Arc<SearchService>,
    ) -> Result<Self> {
        Ok(Self { ctx, llm, fs, graph, search })
    }

    pub async fn run_analysis<F>(&self, file_path: &str, start_line: usize, end_line: usize, mut callback: F) -> Result<String> 
    where F: FnMut(CortexEvent) + Send + Sync + 'static
    {
        callback(CortexEvent::Thought(format!("Calculating precise diff impact detection for {}:{}-{}...", file_path, start_line, end_line)));
        
        let diff = FileDiff {
            path: PathBuf::from(file_path),
            changed_ranges: vec![(start_line, end_line)],
        };
        
        let affected_symbols = {
            let mut analyzer = DiffAnalyzer::new()?;
            analyzer.find_affected_symbols(&[diff], &self.ctx.root)?
        };

        if affected_symbols.is_empty() {
             return Ok(format!("Analysis COMPLETE.\n\nNo code symbols were found in the changed range {}:{}-{}. This might be a change to comments, whitespace, or non-code files.\n\n**Risk Level:** Low.", file_path, start_line, end_line));
        }

        let symbol_context: Vec<String> = affected_symbols.iter()
            .map(|s| format!("- {} ({})", s.name, s.kind))
            .collect();

        callback(CortexEvent::Thought(format!("Identified modified symbols: {}", affected_symbols.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", "))));

        let mut agent_ctx = AgentContext::new(
            self.ctx.clone(),
            self.search.clone(),
            self.ctx.config.agent.clone(),
        );

        agent_ctx.register_tool(Arc::new(ReadFilesTool::new(self.fs.clone())));
        agent_ctx.register_tool(Arc::new(ViewFileOutlineTool::new(self.fs.clone())));
        agent_ctx.register_tool(Arc::new(ViewCodeItemTool::new(self.fs.clone())));

        agent_ctx.register_tool(Arc::new(FindUsagesTool::new(self.graph.clone())));
        let graph_impl = self.graph.clone(); // Clone for InspectGraphTool
        agent_ctx.register_tool(Arc::new(InspectGraphTool::new(graph_impl, self.ctx.clone())));

        let search_impl = Arc::new(Search::new(self.ctx.clone(), self.search.clone()));
        agent_ctx.register_tool(Arc::new(SearchCodeTool::new(search_impl)));

        let mut cortex = Cortex::new(agent_ctx, self.llm.clone());

        let prompt = format!(
r#"You are an expert Senior Staff Engineer doing a Code Review / Impact Analysis.

# THE CHANGE
The user has modified the following file: `{file_path}` (lines {start_line}-{end_line}).
Static analysis indicates the following symbols were modified:
{symbols}

# YOUR MISSION
Analyze the **semantic impact** and **risk** of this change. Don't just list callers; explain strictly *how* they are affected.

# STRATEGY
1. **Verify Context**: Read the modified code in `{file_path}` to understand the *nature* of the change (bug fix? refactor? breaking change?).
2. **Trace Dependencies**: Use `find_usages` or `inspect_graph` on the modified symbols to find who calls them.
3. **Analyze Call Sites**: REQUIRED: Read the code of at least the most critical call sites to see if the change breaks assumptions (e.g., nullability, arguments, side effects).
4. **Conclusion**: Rate the Risk (Low/Medium/High) and explain why.

# OUTPUT FORMAT
Return a Markdown report.
"#,
            file_path = file_path,
            start_line = start_line,
            end_line = end_line,
            symbols = symbol_context.join("\n")
        );

        let result = cortex.run(&prompt, callback).await?;

        Ok(result)
    }
}
