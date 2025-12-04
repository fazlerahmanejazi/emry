use anyhow::{Context, Result};
use emry_agent::cortex::context::AgentContext;
use emry_agent::cortex::Cortex;
use emry_agent::cortex::tools::{
    fs::{ListFilesTool, ReadFileTool, ViewFileOutlineTool, ViewCodeItemTool, ViewCodebaseMapTool},
    graph::{InspectGraphTool, FindReferencesTool, GoToDefinitionTool, GetTypeDefinitionTool},
    search::SearchCodeTool,
    workflows::{ReadFilesTool as ReadFilesMacroTool, ExploreModuleTool, FindUsagesTool},
    architecture::DescribeArchitectureTool,
    impact::AnalyzeImpactTool,
    focus::FocusTool,
};
use emry_agent::llm::OpenAIProvider;
use emry_config::AgentConfig;
use emry_agent::project as agent_context;
use emry_engine::search::service::SearchService;
use emry_agent::ops::fs::FsTool;
use emry_agent::ops::graph::GraphTool;
use emry_agent::ops::search::Search;
use emry_agent::ops::architecture::ArchitectureTool;

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

    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY environment variable not set")?;
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let llm = OpenAIProvider::new(model, api_key, 60)?;
    
    let _ = ctx.surreal_store.clone()
        .ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized in context"))?;
    let search_service = Arc::new(SearchService::new(
        ctx.surreal_store.clone().unwrap(),
        ctx.embedder.clone(),
    ));
    
    let mut agent_ctx = AgentContext::new(
        ctx.clone(),
        search_service.clone(),
        AgentConfig::default(),
    );

    let search_impl = Arc::new(Search::new(ctx.clone(), search_service.clone()));
    let search_tool = SearchCodeTool::new(search_impl);

    let graph_impl = Arc::new(GraphTool::new(ctx.clone()));
    let graph_tool = InspectGraphTool::new(graph_impl.clone(), ctx.clone());
    let find_refs_tool = FindReferencesTool::new(graph_impl.clone());
    let goto_def_tool = GoToDefinitionTool::new(graph_impl.clone());
    let get_type_def_tool = GetTypeDefinitionTool::new(graph_impl.clone());
    
    let fs_impl = Arc::new(FsTool::new(ctx.clone()));

    let arch_impl = Arc::new(ArchitectureTool::new(ctx.clone()));
    let arch_tool = DescribeArchitectureTool::new(
        arch_impl.clone(), 
        fs_impl.clone(), 
        llm.clone(), 
        ctx.clone(), 
        search_service.clone()
    );

    let impact_tool = AnalyzeImpactTool::new(
        ctx.clone(),
        llm.clone(),
        fs_impl.clone(),
        graph_impl.clone(),
        search_service.clone()
    )?;
    
    let focus_tool = FocusTool::new(ctx.clone())?;

    let fs_tool = ReadFileTool::new(fs_impl.clone());
    let list_files_tool = ListFilesTool::new(fs_impl.clone());
    let view_outline_tool = ViewFileOutlineTool::new(fs_impl.clone());
    let view_code_item_tool = ViewCodeItemTool::new(fs_impl.clone());
    let view_codebase_map_tool = ViewCodebaseMapTool::new(fs_impl.clone());

    let read_files_macro_tool = ReadFilesMacroTool::new(fs_impl.clone());
    let explore_module_tool = ExploreModuleTool::new(fs_impl.clone());
    let find_usages_tool = FindUsagesTool::new(graph_impl.clone());

    agent_ctx.register_tool(Arc::new(search_tool));
    agent_ctx.register_tool(Arc::new(graph_tool));
    agent_ctx.register_tool(Arc::new(fs_tool));
    agent_ctx.register_tool(Arc::new(list_files_tool));
    agent_ctx.register_tool(Arc::new(view_outline_tool));
    agent_ctx.register_tool(Arc::new(view_code_item_tool));
    agent_ctx.register_tool(Arc::new(view_codebase_map_tool));
    agent_ctx.register_tool(Arc::new(find_refs_tool));
    agent_ctx.register_tool(Arc::new(goto_def_tool));
    agent_ctx.register_tool(Arc::new(get_type_def_tool));
    agent_ctx.register_tool(Arc::new(read_files_macro_tool));
    agent_ctx.register_tool(Arc::new(explore_module_tool));
    agent_ctx.register_tool(Arc::new(find_usages_tool));
    agent_ctx.register_tool(Arc::new(arch_tool));
    agent_ctx.register_tool(Arc::new(impact_tool));
    agent_ctx.register_tool(Arc::new(focus_tool));



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
