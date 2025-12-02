use anyhow::Result;
use clap::Parser;
use emry_agent::project as agent_context;
use emry_agent::project::embedder::get_embedding_dimension;
use emry_store::SurrealStore;
use std::path::Path;
use std::sync::Arc;
use console::Style;
use super::ui;

#[derive(Parser)]
pub struct InspectArgs {
    /// The ID of the chunk to inspect (e.g., chunk:uuid, symbol:path::name)
    pub id: String,
}

pub async fn handle_inspect(args: InspectArgs, config_path: Option<&Path>) -> Result<()> {
    ui::print_header(&format!("Inspecting: {}", args.id));

    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    
    // Initialize SurrealStore if not already in context
    let surreal_store = if let Some(store) = ctx.surreal_store {
        store
    } else {
        let surreal_path = ctx.index_dir.join("surreal.db");
        let vector_dim = get_embedding_dimension(&ctx.config.embedding);
        Arc::new(SurrealStore::new(&surreal_path, vector_dim).await?)
    };

    // 1. Try as Chunk
    if let Ok(Some(chunk)) = surreal_store.get_chunk(&args.id).await {
        ui::print_panel("Type", "Chunk", Style::new().blue(), None);
        
        println!("{} {}", Style::new().dim().apply_to("ID:"), chunk.id.as_ref().map(|t| t.to_string()).unwrap_or_default());
        println!("{} {}", Style::new().dim().apply_to("File:"), chunk.file.to_string());
        println!("{} {}-{}", Style::new().dim().apply_to("Range:"), chunk.start_line, chunk.end_line);
        println!("{} {}", Style::new().dim().apply_to("Has Embedding:"), chunk.embedding.is_some());
        
        println!("\n{}", Style::new().bold().apply_to("Content:"));
        println!("------------------------------------------------");
        println!("{}", chunk.content);
        println!("------------------------------------------------");
        
        return Ok(());
    }

    // 2. Try as generic Node (Symbol/File)
    if let Ok(Some(node)) = surreal_store.get_node(&args.id).await {
        let kind_style = match node.kind.as_str() {
            "file" => Style::new().yellow(),
            "symbol" => Style::new().cyan(),
            _ => Style::new().white(),
        };
        
        ui::print_panel("Type", &node.kind, kind_style, None);
        
        println!("{} {}", Style::new().dim().apply_to("ID:"), node.id);
        println!("{} {}", Style::new().dim().apply_to("Label:"), node.label);
        println!("{} {}", Style::new().dim().apply_to("File Path:"), node.file_path);
        
        return Ok(());
    }

    ui::print_error(&format!("Node not found: {}", args.id));
    Ok(())
}
