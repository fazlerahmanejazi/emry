use anyhow::Result;
use clap::{Parser, ValueEnum};
use emry_agent::project as agent_context;
use emry_agent::project::types::GraphSubgraph;
use emry_agent::ops::graph::{GraphTool, GraphDirection as ToolGraphDirection};
use std::path::Path;
use std::sync::Arc;

#[derive(Parser)]
pub struct GraphArgs {
    /// The node ID to start from (e.g., a file path, chunk ID, or symbol ID)
    #[arg(long)]
    pub node: String,
    /// Filter by file path (e.g., "cli/src/commands" or "ask.rs")
    #[arg(long)]
    pub file: Option<String>,
    /// Direction of traversal (incoming, outgoing)
    #[arg(long, value_enum, default_value_t = GraphDirection::Outgoing)]
    pub direction: GraphDirection,
    /// Maximum number of hops (depth) to traverse
    #[arg(long, default_value_t = 1)]
    pub max_hops: u8,
    /// Filter by relation kinds (e.g., calls, imports, defines)
    #[arg(long)]
    pub kinds: Vec<String>,
    /// Filter by node kind (file, symbol, chunk) to resolve ambiguity
    #[arg(long)]
    pub kind: Option<String>,
    /// Output in JSON format
    #[arg(long, default_value_t = false)]
    pub json: bool,
    /// Show chunk nodes (hidden by default to reduce noise)
    #[arg(long, default_value_t = false)]
    pub show_chunks: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum GraphDirection {
    Incoming,
    Outgoing,
    Both,
}

impl From<GraphDirection> for ToolGraphDirection {
    fn from(d: GraphDirection) -> Self {
        match d {
            GraphDirection::Incoming => ToolGraphDirection::In,
            GraphDirection::Outgoing => ToolGraphDirection::Out,
            GraphDirection::Both => ToolGraphDirection::Both,
        }
    }
}



pub async fn handle_graph(args: GraphArgs, config_path: Option<&Path>) -> Result<()> {
    use super::ui;

    ui::print_header(&format!("Graph: {}", args.node));

    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    
    if ctx.surreal_store.is_none() {
        return Err(anyhow::anyhow!("SurrealStore not initialized. Run 'emry index' first."));
    }
    let ctx = Arc::new(ctx);

    let graph_tool = GraphTool::new(ctx.clone());

    let direction = args.direction.into();
    let result = graph_tool.graph(&args.node, direction, args.max_hops as usize, args.file.as_deref()).await;

    match result {
        Ok(graph_res) => {
            if let Some(candidates) = graph_res.candidates {
                if args.json {
                    println!("{}", serde_json::json!({
                        "disambiguation": true,
                        "candidates": candidates
                    }));
                    return Ok(());
                }
                
                use dialoguer::{theme::ColorfulTheme, Select};

                println!("\nFound {} symbols matching '{}':", candidates.len(), args.node);
                
                let selections: Vec<String> = candidates.iter()
                    .map(|c| format!("{} ({})\n   File: {}\n   ID: {}", c.label, c.kind, c.file_path, c.id))
                    .collect();

                let selection = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Select a symbol to inspect")
                    .default(0)
                    .items(&selections)
                    .interact_opt()?;

                if let Some(idx) = selection {
                    let selected = &candidates[idx];
                    println!("\nQuerying: {}\n", selected.id);
                    
                    let final_result = graph_tool.graph(
                        &selected.id, 
                        direction, 
                        args.max_hops as usize,
                        None
                    ).await?;
                    
                    process_and_output(final_result.subgraph, &selected.label, &args.kinds, args.json)?;
                    return Ok(());
                } else {
                    println!("Selection cancelled");
                    return Ok(());
                }
            }
            
            process_and_output(graph_res.subgraph, &args.node, &args.kinds, args.json)?;
        }
        Err(e) => {
            if args.json {
                println!("{}", serde_json::json!({ "error": e.to_string() }));
            } else {
                ui::print_error(&format!("Error: {}", e));
            }
        }
    }

    Ok(())
}

fn process_and_output(
    mut subgraph: GraphSubgraph,
    source_label: &str,
    kinds: &[String],
    json: bool,
) -> Result<()> {
    if !kinds.is_empty() {
        subgraph.edges.retain(|e| kinds.contains(&e.kind));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&subgraph)?);
    } else {
        if subgraph.nodes.is_empty() {
            println!("No nodes found for '{}'", source_label);
        } else {
            print_subgraph(&subgraph, source_label);
        }
    }
    Ok(())
}

fn print_subgraph(subgraph: &GraphSubgraph, source_node: &str) {
    use console::Style;
    use std::collections::HashMap;

    let node_labels: HashMap<&str, &str> = subgraph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.label.as_str()))
        .collect();

    let neighbors: Vec<_> = subgraph
        .nodes
        .iter()
        .filter(|n| n.id != source_node && n.label != source_node) // Filter out start node if present
        .collect();

    println!(
        "\nFound {} neighbors for '{}':",
        neighbors.len(),
        Style::new().bold().cyan().apply_to(source_node)
    );

    for (i, node) in neighbors.iter().enumerate() {
        println!(
            "{} {} ({})",
            Style::new().dim().apply_to(format!("{}.", i + 1)),
            Style::new().bold().apply_to(&node.label),
            Style::new().dim().apply_to(&node.id)
        );
    }

    if !subgraph.edges.is_empty() {
        println!("\nEdges:");
        for edge in &subgraph.edges {
            let kind_style = match edge.kind.as_str() {
                "calls" => Style::new().yellow(),
                "imports" => Style::new().magenta(),
                "defines" => Style::new().blue(),
                _ => Style::new().white(),
            };

            let source_label = node_labels
                .get(edge.source.as_str())
                .copied()
                .unwrap_or(edge.source.as_str());
            let target_label = node_labels
                .get(edge.target.as_str())
                .copied()
                .unwrap_or(edge.target.as_str());

            println!(
                "  {} {} {}",
                format!(
                    "{} ({})",
                    Style::new().bold().apply_to(source_label),
                    Style::new().dim().apply_to(&edge.source)
                ),
                kind_style.apply_to(format!("-[{}]->", edge.kind)),
                format!(
                    "{} ({})",
                    Style::new().bold().apply_to(target_label),
                    Style::new().dim().apply_to(&edge.target)
                )
            );
        }
    }
}
