use anyhow::{Context, Result};
use emry_agent::cortex::context::AgentContext;
use emry_agent::cortex::Cortex;
use emry_agent::cortex::tools::{
    fs::{ListFilesTool, ReadFileTool},
    graph::InspectGraphTool,
    search::SearchCodeTool,
};
use emry_agent::llm::OpenAIProvider;
use emry_config::AgentConfig;
use emry_agent::project as agent_context;
use emry_engine::search::service::SearchService;
use emry_agent::ops::fs::FsTool;
use emry_agent::ops::graph::GraphTool;
use emry_agent::ops::search::Search;
use std::path::Path;
use std::sync::Arc;

use super::utils::render_markdown_answer;

pub async fn handle_ask(query: String, verbose: bool, config_path: Option<&Path>) -> Result<()> {
    use super::ui;
    use console::Style;

    if verbose {
        ui::print_header(&format!("Query: {}", query));
    }

    let ctx = Arc::new(agent_context::RepoContext::from_env(config_path).await?);
    
    // Initialize SurrealStore & SearchService

    // Reuse store from context if available, or error out
    let surreal_store = ctx.surreal_store.clone()
        .ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized in context"))?;
    let search_service = Arc::new(SearchService::new(surreal_store.clone(), ctx.embedder.clone()));
    


    // Initialize AgentContext
    let mut agent_ctx = AgentContext::new(
        ctx.clone(),
        search_service.clone(),
        AgentConfig::default(),
    );

    // Initialize tools
    let search_impl = Arc::new(Search::new(ctx.clone(), search_service.clone()));
    let search_tool = SearchCodeTool::new(search_impl);

    let graph_impl = Arc::new(GraphTool::new(ctx.clone()));
    let graph_tool = InspectGraphTool::new(graph_impl, ctx.clone());

    let fs_impl = Arc::new(FsTool::new(ctx.clone()));
    let fs_tool = ReadFileTool::new(fs_impl.clone());
    let list_files_tool = ListFilesTool::new(fs_impl.clone());

    agent_ctx.register_tool(Arc::new(search_tool));
    agent_ctx.register_tool(Arc::new(graph_tool));
    agent_ctx.register_tool(Arc::new(fs_tool));
    agent_ctx.register_tool(Arc::new(list_files_tool));

    // Initialize LLM
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY environment variable not set")?;
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let llm = OpenAIProvider::new(model, api_key, 60)?;

    let mut cortex = Cortex::new(agent_ctx, llm);

    let answer = cortex.run(&query, |event| {
        if verbose {
            match event {
                emry_agent::cortex::CortexEvent::StepStart(step) => {
                    println!("\n{}", Style::new().dim().apply_to(format!("── Step {} ──", step)));
                }
                emry_agent::cortex::CortexEvent::Thought(thought) => {
                    ui::print_panel("Thought", &thought, Style::new().green(), Some(Style::new().dim()));
                }
                emry_agent::cortex::CortexEvent::ToolCall { name, args } => {
                    ui::print_panel("Tool Call", &format!("{}({})", name, args), Style::new().yellow(), Some(Style::new().dim()));
                }
                emry_agent::cortex::CortexEvent::ToolResult { name: _, result } => {
                     let truncated = if result.len() > 300 {
                         format!("{}...", &result[..300])
                     } else {
                         result
                     };
                     ui::print_panel("Observation", &truncated, Style::new().blue(), Some(Style::new().dim()));
                }
            }
        }
    }).await?;
    
    ui::print_header("Final Answer");
    println!("{}", render_markdown_answer(&answer));

    Ok(())
}
