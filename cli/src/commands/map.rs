use anyhow::Result;
use emry_agent::ops::fs::FsTool;
use emry_agent::project::RepoContext;
use std::path::Path;
use std::sync::Arc;

pub async fn handle_codebase_map(depth: usize, verbose: bool, config_path: Option<&Path>) -> Result<()> {
    use super::ui;
    use console::Style;

    ui::print_header("Codebase Map");

    let ctx = Arc::new(RepoContext::from_env(config_path).await?);
    let fs_tool = FsTool::new(ctx.clone());

    if verbose {
        ui::print_panel("Step", "Generating codebase map...", Style::new().blue(), Some(Style::new().dim()));
    }

    let map = fs_tool.generate_codebase_map(depth)?;
    
    println!("{}", map);

    Ok(())
}
