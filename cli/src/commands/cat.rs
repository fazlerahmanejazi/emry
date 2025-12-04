use anyhow::Result;
use emry_agent::ops::fs::FsTool;
use emry_agent::project::RepoContext;
use std::path::Path;
use std::sync::Arc;

pub async fn handle_cat(paths: Vec<String>, config_path: Option<&Path>) -> Result<()> {
    let ctx = Arc::new(RepoContext::from_env(config_path).await?);
    let fs_tool = FsTool::new(ctx.clone());

    let path_bufs: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
    let results = fs_tool.read_files_concurrent(path_bufs).await;

    for (path, content) in results {
        println!("--- {} ---", path.display());
        println!("{}", content);
        println!();
    }

    Ok(())
}
