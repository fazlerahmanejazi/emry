use anyhow::{Context, Result};
use emry_agent::llm::OpenAIProvider;
use emry_agent::project as agent_context;
use std::path::Path;
use std::sync::Arc;
use emry_engine::search::service::SearchService;
use emry_agent::ops::fs::FsTool;
use emry_agent::workflows::functional::FunctionalWorkflow;

use super::utils::render_markdown_answer;

pub async fn handle_explain(verbose: bool, config_path: Option<&Path>) -> Result<()> {
    use super::ui;
    use console::Style;
    
    ui::print_header("Functional Project Overview");

    let ctx = Arc::new(agent_context::RepoContext::from_env(config_path).await?);

    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY environment variable not set")?;
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let llm = OpenAIProvider::new(model, api_key, 60)?;
    
    let store = ctx.surreal_store.clone()
        .ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized. Run 'emry index' first."))?;
    
    let search_service = Arc::new(SearchService::new(
        store,
        ctx.embedder.clone(),
    ));

    let fs_tool = Arc::new(FsTool::new(ctx.clone()));
    
    let workflow = FunctionalWorkflow::new(
        fs_tool, 
        llm, 
        ctx.clone(), 
        search_service
    );

    ui::print_panel("Running", "Analyzing project functionality and capabilities...", console::Style::new().yellow(), None);
    
    let report = workflow.run_analysis(move |event| {
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
    
    ui::print_header("Functional Overview");
    println!("{}", render_markdown_answer(&report));

    Ok(())
}
