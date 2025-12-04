use anyhow::Result;

use std::path::Path;

pub async fn handle_debug(config_path: Option<&Path>) -> Result<()> {
    use super::ui;
    use console::Style;

    ui::print_header("Debug: Database Stats");

    let ctx = emry_agent::project::context::RepoContext::from_env(config_path).await?;
    
    ui::print_panel("Info", &format!("Connecting to store at: {}", ctx.index_dir.display()), Style::new().dim(), None);
    
    let store = ctx.surreal_store.ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized"))?;
    let db = store.db();
    
    let queries = [
        ("Files", "SELECT count() FROM file GROUP ALL"),
        ("Symbols", "SELECT count() FROM symbol GROUP ALL"),
        ("Chunks", "SELECT count() FROM chunk GROUP ALL"),
        ("Imports (Edges)", "SELECT count() FROM imports GROUP ALL"),
        ("Calls (Edges)", "SELECT count() FROM calls GROUP ALL"),
        ("Defines (Edges)", "SELECT count() FROM defines GROUP ALL"),
        ("Contains (Edges)", "SELECT count() FROM contains GROUP ALL"),
    ];
    
    for (label, q) in queries {
        let mut res = db.query(q).await?;
        let result: Option<serde_json::Value> = res.take(0)?;
        
        let count = if let Some(val) = result {
            val.get("count").and_then(|c| c.as_u64()).unwrap_or(0)
        } else {
            0
        };
        
        println!("{}: {}", label, count);
    }
    
    Ok(())
}
