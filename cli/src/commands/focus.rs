use anyhow::Result;
use emry_agent::ops::context::SmartContext;
use emry_agent::project as agent_context;
use std::path::Path;
use std::sync::Arc;
use super::utils::render_markdown_answer;

pub async fn handle_focus(topic: String, verbose: bool, config_path: Option<&Path>) -> Result<()> {
    use super::ui;
    
    ui::print_header(&format!("Smart Focus: {}", topic));

    let ctx = Arc::new(agent_context::RepoContext::from_env(config_path).await?);

    let smart_context = SmartContext::new(ctx.clone())?;
    
    ui::print_panel("Running", "Gathering context...", console::Style::new().yellow(), None);
    
    let report = smart_context.focus(&topic, |step| {
        if verbose {
            ui::print_panel("Step", &step, console::Style::new().blue(), Some(console::Style::new().dim()));
        }
    }).await?;
    
    ui::print_header("Context Report");
    println!("{}", render_markdown_answer(&report));

    Ok(())
}
