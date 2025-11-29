use anyhow::{Context, Result};
use emry_core::stack_graphs::debugger::GraphDebugger;
use emry_core::stack_graphs::manager::StackGraphManager;
use std::path::PathBuf;
use super::utils::current_branch;

pub async fn handle_explain(
    location: String,
    json: bool,
) -> Result<()> {
    let root = std::env::current_dir()?;

    // 1. Parse location (file:line:col)
    let parts: Vec<&str> = location.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid location format. Expected file:line:col, got '{}'", location);
    }
    let file_path = PathBuf::from(parts[0]);
    let file_path = if file_path.is_absolute() {
        file_path
    } else {
        root.join(file_path)
    };
    // canonicalize might fail if file doesn't exist, but here we expect it to exist.
    // However, stack-graphs might store it as "canonical" or just "absolute from root".
    // In index.rs, we use `scanned_files` which uses `walkdir` or similar.
    // Usually it's absolute.
    // Let's try to canonicalize if possible to match exactly.
    let file_path = std::fs::canonicalize(&file_path).unwrap_or(file_path);
    let file_path_str = file_path.to_string_lossy();

    let line: usize = parts[1].parse().context("Invalid line number")?;
    let col: usize = parts[2].parse().context("Invalid column number")?;

    // 2. Load StackGraphManager
    let branch = current_branch();
    let index_dir = root.join(".codeindex").join("branches").join(branch);
    let stack_graph_path = index_dir.join("stack_graph.bin");

    if !stack_graph_path.exists() {
        anyhow::bail!("Stack graph not found at {}. Run 'emry index' first.", stack_graph_path.display());
    }

    let manager = StackGraphManager::new(stack_graph_path)?;
    let debugger = GraphDebugger::new(&manager);

    // 3. Trace Reference
    let trace = debugger.trace_reference(&file_path_str, line, col)?;

    // 4. Output
    if json {
        let json_output = serde_json::to_string_pretty(&trace)?;
        println!("{}", json_output);
    } else {
        println!("Reference: {} ({}:{}:{})", 
            trace.reference.symbol.as_deref().unwrap_or("?"),
            trace.reference.file, trace.reference.line, trace.reference.col
        );

        if let Some(err) = &trace.error {
            println!("Error: {}", err);
        }

        if trace.paths.is_empty() {
            println!("No resolution paths found.");
        } else {
            for (i, path) in trace.paths.iter().enumerate() {
                println!("\nPath {}:", i + 1);
                for step in path {
                    let symbol = step.node.symbol.as_deref().unwrap_or("");
                    let location = format!("{}:{}:{}", step.node.file, step.node.line, step.node.col);
                    println!("  -> [{}] {} ({})", step.kind, symbol, location);
                }
            }
        }
    }

    Ok(())
}
