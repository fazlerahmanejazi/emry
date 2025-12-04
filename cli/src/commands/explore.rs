use anyhow::Result;
use emry_agent::ops::fs::FsTool;
use emry_agent::project::RepoContext;
use std::path::Path;
use std::sync::Arc;

pub async fn handle_explore(path: String, depth: usize, config_path: Option<&Path>) -> Result<()> {
    let ctx = Arc::new(RepoContext::from_env(config_path).await?);
    let fs_tool = FsTool::new(ctx.clone());

    let result = fs_tool.explore_module(&path, depth).await?;
    println!("{}", result);

    Ok(())
}
