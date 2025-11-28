use anyhow::{Context, Result};
use stack_graphs::graph::StackGraph;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// Load a StackGraph from disk.
/// Returns a new empty graph if the file does not exist.
pub fn load_graph(path: &Path) -> Result<StackGraph> {
    if !path.exists() {
        return Ok(StackGraph::new());
    }

    let file = File::open(path).with_context(|| format!("Failed to open stack graph file: {:?}", path))?;
    let reader = BufReader::new(file);
    
    // Use the serializable wrapper provided by stack-graphs
    let serializable: stack_graphs::serde::StackGraph = bincode::deserialize_from(reader)
        .with_context(|| format!("Failed to deserialize stack graph from {:?}", path))?;
    
    let mut graph = StackGraph::new();
    serializable.load_into(&mut graph).context("Failed to load serializable graph into StackGraph")?;
        
    Ok(graph)
}

/// Save a StackGraph to disk.
pub fn save_graph(graph: &StackGraph, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    let file = File::create(path).with_context(|| format!("Failed to create stack graph file: {:?}", path))?;
    let writer = BufWriter::new(file);
    
    // Create serializable wrapper
    let serializable = stack_graphs::serde::StackGraph::from_graph(graph);
    
    bincode::serialize_into(writer, &serializable)
        .with_context(|| format!("Failed to serialize stack graph to {:?}", path))?;
        
    Ok(())
}
