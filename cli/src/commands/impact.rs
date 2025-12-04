use anyhow::{Context, Result};
use emry_agent::cortex::tools::impact::AnalyzeImpactTool;

use emry_agent::project as agent_context;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde_json::json;
use super::utils::render_markdown_answer;

use emry_agent::llm::OpenAIProvider;
use emry_engine::search::service::SearchService;
use emry_agent::ops::fs::FsTool;
use emry_agent::ops::graph::GraphTool;

pub async fn handle_impact(file_path: PathBuf, start_line: usize, end_line: usize, verbose: bool, config_path: Option<&Path>) -> Result<()> {
    use super::ui;
    use console::Style;
    use emry_agent::cortex::CortexEvent;
    
    ui::print_header(&format!("Impact Analysis: {}:{}-{}", file_path.display(), start_line, end_line));

    let ctx = Arc::new(agent_context::RepoContext::from_env(config_path).await?);

    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY environment variable not set")?;
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let llm = OpenAIProvider::new(model, api_key, 60)?;

    let store = ctx.surreal_store.clone()
        .ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized. Run 'emry index' first."))?;
    
    let search = Arc::new(SearchService::new(
        store,
        ctx.embedder.clone(),
    ));

    let fs = Arc::new(FsTool::new(ctx.clone()));
    let graph = Arc::new(GraphTool::new(ctx.clone()));

    let impact_tool = AnalyzeImpactTool::new(
        ctx.clone(),
        llm,
        fs,
        graph,
        search
    )?;

    let _args = json!({
        "file_path": file_path.to_string_lossy(),
        "start_line": start_line,
        "end_line": end_line
    });
    
    ui::print_panel("Running", "Analyze semantic impact...", console::Style::new().yellow(), None);
    
    let report = impact_tool.run_analysis(&file_path.to_string_lossy(), start_line, end_line, move |event| {
        if verbose {
            match event {
                CortexEvent::StepStart(step) => {
                    println!("\n{}", Style::new().dim().apply_to(format!("── Step {} ──", step)));
                }
                CortexEvent::Thought(thought) => {
                    ui::print_panel("Thought", &thought, Style::new().green(), Some(Style::new().dim()));
                }
                CortexEvent::ToolCall { name, args } => {
                    ui::print_panel("Tool Call", &format!("{}({})", name, args), Style::new().yellow(), Some(Style::new().dim()));
                }
                CortexEvent::ToolResult { name: _, result } => {
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
    
    ui::print_header("Impact Report");
    println!("{}", render_markdown_answer(&report));

    Ok(())
}
