use crate::ops::fs::FsTool;
use crate::ops::search::Search;
use crate::ops::graph::GraphTool;
use anyhow::Result;

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

pub struct FunctionalWorkflow {
    fs: Arc<FsTool>,
    llm: OpenAIProvider,
    ctx: Arc<RepoContext>,
    search: Arc<SearchService>,
}

impl FunctionalWorkflow {
    pub fn new(
        fs: Arc<FsTool>, 
        llm: OpenAIProvider,
        ctx: Arc<RepoContext>,
        search: Arc<SearchService>,
    ) -> Self {
        Self { fs, llm, ctx, search }
    }

    pub async fn run_analysis<F>(&self, callback: F) -> Result<String>
    where F: FnMut(CortexEvent) + Send + Sync + 'static 
    {
        self.run_agentic_analysis(callback).await
    }

    async fn run_agentic_analysis<F>(&self, mut callback: F) -> Result<String>
    where F: FnMut(CortexEvent) + Send + Sync + 'static 
    {
        callback(CortexEvent::Thought("Initializing Functional Analysis Agent...".to_string()));

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

        let prompt = r#"You are an expert Technical Product Manager and Systems Architect.
Your goal is to explain EXACTLY what this software does and HOW it works.

# INVESTIGATION PLAN
1. **Identify Capabilities**: Look at user-facing interfaces.
    - Scan for CLI definitions, API routes, or UI entry points in any language (e.g., Rust, TS, Python, Go).
    - Read configuration files or manifests (like `package.json`, `Cargo.toml`) to infer intended usage.
    - Check `README` or documentation.
2. **Trace Workflows**: For each key capability, trace the "Happy Path" from input to output.
    - How does data flow? 
    - what are the major transformations?
3. **Explain Internals**: Reveal the "magic" under the hood.
    - What key data structures or algorithms drive this feature?
    - How is state managed or persisted?
    - Are there interesting design patterns used?

# OUTPUT FORMAT
Produce a "Functional Deep Dive" report in Markdown covering:
- **Project Mission**: What problem does this solve?
- **Key Capabilities**: List top 3-5 major features.
- **Deep Dives**: For each feature, provide:
    - *User View*: How it's used.
    - *Internal View*: How it works (key components, logic, storage).
- **External Interfaces**: APIs, CLI flags, configuration.

Be specific and cite file names where relevant.
"#;
        
        cortex.run(prompt, callback).await
    }
}
