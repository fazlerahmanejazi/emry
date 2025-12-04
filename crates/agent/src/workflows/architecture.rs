use crate::ops::architecture::ArchitectureTool;
use crate::ops::fs::FsTool;
use crate::ops::search::Search;
use crate::ops::graph::GraphTool;
use anyhow::Result;
use emry_core::traits::LLM;
use emry_engine::search::service::SearchService;
use crate::project::context::RepoContext;
use crate::cortex::{Cortex, CortexEvent};
use crate::cortex::context::AgentContext;
use crate::llm::OpenAIProvider;
use std::sync::Arc;

use crate::cortex::tools::{
    fs::{ListFilesTool, ViewCodebaseMapTool, ViewFileOutlineTool},
    graph::InspectGraphTool,
    search::SearchCodeTool,
    workflows::{ReadFilesTool as ReadFilesMacroTool},
};

pub struct ArchitectureWorkflow {
    inner: Arc<ArchitectureTool>,
    fs: Arc<FsTool>,
    llm: OpenAIProvider,
    ctx: Arc<RepoContext>,
    search: Arc<SearchService>,
}

impl ArchitectureWorkflow {
    pub fn new(
        inner: Arc<ArchitectureTool>, 
        fs: Arc<FsTool>, 
        llm: OpenAIProvider,
        ctx: Arc<RepoContext>,
        search: Arc<SearchService>,
    ) -> Self {
        Self { inner, fs, llm, ctx, search }
    }

    pub async fn run_analysis<F>(&self, mode: &str, callback: F) -> Result<String> 
    where F: FnMut(CortexEvent) + Send + Sync + 'static
    {
        if mode == "deep" {
            self.run_agentic_analysis(callback).await
        } else {
            self.run_fast_analysis(callback).await
        }
    }

    async fn run_agentic_analysis<F>(&self, mut callback: F) -> Result<String>
    where F: FnMut(CortexEvent) + Send + Sync + 'static 
    {
        callback(CortexEvent::Thought("Initializing Agentic Architecture Analyst...".to_string()));

        let mut agent_ctx = AgentContext::new(
            self.ctx.clone(),
            self.search.clone(),
            self.ctx.config.agent.clone(),
        );

        let search_impl = Arc::new(Search::new(self.ctx.clone(), self.search.clone()));
        agent_ctx.register_tool(Arc::new(SearchCodeTool::new(search_impl)));

        let graph_impl = Arc::new(GraphTool::new(self.ctx.clone()));
        agent_ctx.register_tool(Arc::new(InspectGraphTool::new(graph_impl.clone(), self.ctx.clone())));

        agent_ctx.register_tool(Arc::new(ListFilesTool::new(self.fs.clone())));
        agent_ctx.register_tool(Arc::new(ViewCodebaseMapTool::new(self.fs.clone())));
        agent_ctx.register_tool(Arc::new(ViewFileOutlineTool::new(self.fs.clone())));
        agent_ctx.register_tool(Arc::new(ReadFilesMacroTool::new(self.fs.clone())));

        let mut cortex = Cortex::new(agent_ctx, self.llm.clone());

        let prompt = r#"You are an expert Software Architect assigned to analyze this codebase.
Your goal is to produce a comprehensive "Deep Research" architecture report.

# STRATEGIC PLAYBOOK
1. **Reconnaissance**:
    - Identify project type and dependencies (Cargo.toml, package.json, etc).
    - Understand directory structure and key modules.
2. **Investigation**:
    - Analyze responsibility of key crates/modules.
    - Trace key data flows.
3. **Synthesis**:
    - Describe High-Level Architecture, Core Domain, and Design Patterns.
    - Note technical stack details.

# OUTPUT FORMAT
Return the final report in Markdown. Be specific.
"#;
        
        cortex.run(prompt, callback).await
    }

    async fn run_fast_analysis<F>(&self, mut callback: F) -> Result<String> 
    where F: FnMut(CortexEvent) + Send + Sync + 'static
    {
        let mut send_step = |msg: String| {
             callback(CortexEvent::Thought(msg));
        };

        send_step("Analyzing module coupling and graph centrality...".to_string());
        let (coupling, central_nodes) = self.inner.analyze_structure().await?;
            
        send_step(format!("Found {} module coupling relationships.", coupling.len()));
        send_step(format!("Identified {} central nodes.", central_nodes.len()));
            
        if !central_nodes.is_empty() {
             let top_nodes: Vec<String> = central_nodes.iter().take(3).map(|n| format!("{} ({})", n.label, n.in_degree)).collect();
             send_step(format!("Top central nodes: {}", top_nodes.join(", ")));
        }
            
        send_step("Sampling content from central hubs...".to_string());
        let mut hub_summaries = String::new();
        for node in central_nodes.iter().take(3) {
             send_step(format!("Reading content for: {}", node.file_path));
             let content = self.fs.read_file_span(std::path::Path::new(&node.file_path), 1, 50).unwrap_or_else(|_| "Error reading file".to_string());
             hub_summaries.push_str(&format!("\n--- File: {} (In-Degree: {}) ---\n{}\n", node.file_path, node.in_degree, content));
        }
            
        send_step("Synthesizing architectural description...".to_string());
        
        let prompt = format!(
            "You are an expert Software Architect. Analyze the following data about a codebase and write a comprehensive architectural description.\n\n\
            ## Module Coupling (Who imports whom)\n\
            {:#?}\n\n\
            ## Central Hubs (High In-Degree Nodes)\n\
            {:#?}\n\n\
            ## Key File Samples (Top Hubs)\n\
            {}\n\n\
            ## Instructions\n\
            1. Identify the main architectural layers (e.g., Core, Infrastructure, API).\n\
            2. Describe the data flow and key abstractions.\n\
            3. Identify any potential architectural violations or circular dependencies.\n\
            4. Write in a clear, narrative style.",
            coupling.iter().take(20).collect::<Vec<_>>(),
            central_nodes,
            hub_summaries
        );
            
        let report = self.llm.complete(&prompt).await?;
        Ok(report)
    }
}
