use anyhow::Result;
use clap::{Parser, ValueEnum};
use emry_context as agent_context;
use emry_graph::graph::{CodeGraph, GraphEdgeInfo, GraphNodeInfo, GraphSubgraph};
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;

#[derive(Parser)]
pub struct GraphArgs {
    /// The node ID to start from (e.g., a file path, chunk ID, or symbol ID)
    #[arg(long)]
    pub node: String,
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

pub async fn handle_graph(args: GraphArgs, config_path: Option<&Path>) -> Result<()> {
    let ctx = agent_context::RepoContext::from_env(config_path).await?;
    let graph = ctx.graph.clone();

    // Resolve node ID using the Resolution Layer
    let node_id = {
        let graph = graph.read().unwrap();
        match graph.resolve_node_id(&args.node, args.kind.as_deref()) {
            Ok(id) => id,
            Err(emry_graph::graph::ResolutionError::Ambiguous(query, candidates)) => {
                if args.json {
                    let json_err = json!({
                        "error": "ambiguous_node",
                        "query": query,
                        "candidates": candidates
                    });
                    println!("{}", serde_json::to_string_pretty(&json_err)?);
                    return Ok(());
                }
                eprintln!(
                    "Ambiguous node '{}'. Did you mean one of these?",
                    query
                );
                for (i, c) in candidates.iter().enumerate() {
                    let desc = if c.starts_with("file:") {
                        format!("File Node (ID: {})", c)
                    } else if c.contains(':') { // Symbol usually has path:line:name
                        format!("Symbol Node (ID: {})", c)
                    } else {
                        format!("Chunk Node (ID: {})", c)
                    };
                    eprintln!("{}. {}", i + 1, desc);
                }
                eprintln!("\nTip: Use --kind <file|symbol> to disambiguate.");
                return Ok(())
            }
            Err(emry_graph::graph::ResolutionError::NotFound(query)) => {
                if args.json {
                    let json_err = json!({
                        "error": "node_not_found",
                        "query": query
                    });
                    println!("{}", serde_json::to_string_pretty(&json_err)?);
                    return Ok(());
                }
                eprintln!("Node '{}' not found in the graph.", query);
                return Ok(())
            }
            Err(e) => return Err(e.into()),
        }
    };

    let relation_types: Vec<String> = args
        .kinds
        .clone();

    if !args.json {
        println!(
            "{:?} neighbors for node '{}' (resolved to: '{}', max_hops: {}, kinds: {:?}):",
            args.direction, args.node, node_id, args.max_hops, args.kinds
        );
    }

    // Use neighbors_subgraph for outgoing, manually build for incoming
    let graph_guard = graph.read().unwrap();
    let subgraph = match args.direction {
        GraphDirection::Outgoing => {
             let mut nodes = Vec::new();
             let mut edges = Vec::new();
             
             // Add start node
             if let Some(n) = graph_guard.get_node(&node_id)? {
                 nodes.push(GraphNodeInfo {
                     id: n.id,
                     kind: n.kind,
                     label: n.label,
                     file_path: Some(n.file_path),
                 });
             }
             
             // Get direct neighbors
             let neighbors = graph_guard.get_neighbors(&node_id)?;
             for n in neighbors {
                 // Chunk Skipping Logic
                 if n.kind == "chunk" && !args.show_chunks {
                     // If it's a chunk and we want to hide it, traverse THROUGH it.
                     // Find what this chunk defines (outgoing edges from chunk)
                     let chunk_out_edges = graph_guard.outgoing_edges(&n.id)?;
                     for ce in chunk_out_edges {
                         // We are looking for symbols defined by this chunk
                         if let Some(target_node) = graph_guard.get_node(&ce.target)? {
                             // Add the symbol node
                             nodes.push(GraphNodeInfo {
                                 id: target_node.id.clone(),
                                 kind: target_node.kind,
                                 label: target_node.label,
                                 file_path: Some(target_node.file_path),
                             });
                             // Add a virtual edge from Original Source -> Symbol
                             edges.push(GraphEdgeInfo {
                                 src: node_id.clone(),
                                 dst: target_node.id,
                                 relation: "defines".to_string(), // Virtual relation
                             });
                         }
                     }
                 } else {
                     // Normal behavior (keep node)
                     nodes.push(GraphNodeInfo {
                         id: n.id.clone(),
                         kind: n.kind,
                         label: n.label,
                         file_path: Some(n.file_path),
                     });
                     
                     // Find the edge connecting source -> n
                     // We need to iterate outgoing edges of source to find the one pointing to n
                     let out_edges = graph_guard.outgoing_edges(&node_id)?;
                     for e in out_edges {
                         if e.target == n.id {
                             edges.push(GraphEdgeInfo {
                                 src: e.source,
                                 dst: e.target,
                                 relation: e.kind,
                             });
                         }
                     }
                 }
             }
             
             GraphSubgraph { nodes, edges }
        }
        GraphDirection::Incoming => {
            // Build incoming subgraph manually
            build_incoming_subgraph(&*graph_guard, &node_id, &relation_types, args.max_hops)?
        }
        GraphDirection::Both => {
            // Build both directions
            build_both_subgraph(&*graph_guard, &node_id, &relation_types, args.max_hops)?
        }
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&subgraph)?);
    } else {
        if subgraph.nodes.is_empty() || (subgraph.nodes.len() == 1 && subgraph.nodes[0].id == node_id) {
            println!("No neighbors found for '{}'", node_id);
        } else {
            print_subgraph(&subgraph, &node_id);
        }
    }

    Ok(())
}

fn build_incoming_subgraph(
    graph: &CodeGraph,
    start: &str,
    _relation_types: &[String],
    max_hops: u8,
) -> Result<GraphSubgraph> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut visited = HashSet::new();

    // Add start node
    if let Some(start_node) = graph.get_node(start)? {
        nodes.push(GraphNodeInfo {
            id: start_node.id.clone(),
            kind: start_node.kind.clone(),
            label: start_node.label.clone(),
            file_path: Some(start_node.file_path.clone()),
        });
        visited.insert(start_node.id.clone());
    }

    // BFS for incoming edges
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((start.to_string(), 0u8));

    while let Some((cur, depth)) = queue.pop_front() {
        if depth >= max_hops {
            continue;
        }

        let in_edges = graph.incoming_edges(&cur)?;
        for edge in in_edges {
            // Get source node
            if let Some(mut source_node) = graph.get_node(&edge.source)? {
                // Chunk Skipping Logic (Incoming)
                // If source is a chunk, we want to skip it and show the FILE that contains the chunk.
                // Or, if the chunk defines the symbol, we want to show the FILE/Module that contains the chunk.
                // Actually, for incoming: "Who calls me?"
                // If Chunk A calls Symbol B, we want to show File A calls Symbol B.
                
                let relation = edge.kind.clone();
                let mut final_source_id = source_node.id.clone();

                if source_node.kind == "chunk" {
                    // Find the file that contains this chunk
                    // We can assume the file path in the chunk node is the file path.
                    // But we need the File Node ID.
                    // Usually file node id is "file:<id>".
                    // We can try to find the file node by path? Or traverse outgoing "contains" from file?
                    // Easier: Just look at the `file_path` field of the chunk node, and find the File node for that path.
                    // But `file_path` is a string.
                    // Let's rely on the graph structure: File -[contains]-> Chunk.
                    // So we check incoming edges of the Chunk to find the File.
                    
                    let chunk_in_edges = graph.incoming_edges(&source_node.id)?;
                    for ce in chunk_in_edges {
                        if ce.kind == "contains" {
                            if let Some(file_node) = graph.get_node(&ce.source)? {
                                if file_node.kind == "file" {
                                    source_node = file_node;
                                    final_source_id = source_node.id.clone();
                                    // relation remains "calls" or whatever the chunk did
                                    break;
                                }
                            }
                        }
                    }
                }

                if !visited.contains(&final_source_id) {
                    nodes.push(GraphNodeInfo {
                        id: source_node.id.clone(),
                        kind: source_node.kind.clone(),
                        label: source_node.label.clone(),
                        file_path: Some(source_node.file_path.clone()),
                    });
                    visited.insert(final_source_id.clone());
                    queue.push_back((final_source_id.clone(), depth + 1));
                }

                edges.push(GraphEdgeInfo {
                    src: final_source_id,
                    dst: cur.clone(),
                    relation,
                });
            }
        }
    }

    Ok(GraphSubgraph { nodes, edges })
}

fn build_both_subgraph(
    graph: &CodeGraph,
    start: &str,
    relation_types: &[String],
    max_hops: u8,
) -> Result<GraphSubgraph> {
    // Re-implement logic
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    
    // Outgoing
    if let Some(n) = graph.get_node(start)? {
         nodes.push(GraphNodeInfo {
             id: n.id,
             kind: n.kind,
             label: n.label,
             file_path: Some(n.file_path),
         });
    }
    let out_edges = graph.outgoing_edges(start)?;
    for e in out_edges {
         edges.push(GraphEdgeInfo {
             src: e.source.clone(),
             dst: e.target.clone(),
             relation: e.kind,
         });
         if let Some(n) = graph.get_node(&e.target)? {
             nodes.push(GraphNodeInfo {
                 id: n.id,
                 kind: n.kind,
                 label: n.label,
                 file_path: Some(n.file_path),
             });
         }
    }
    
    let incoming = build_incoming_subgraph(graph, start, relation_types, max_hops)?;

    for node in incoming.nodes {
        if !nodes.iter().any(|n| n.id == node.id) {
            nodes.push(node);
        }
    }

    for edge in incoming.edges {
        if !edges.iter().any(|e| e.src == edge.src && e.dst == edge.dst) {
            edges.push(edge);
        }
    }

    Ok(GraphSubgraph {
        nodes,
        edges,
    })
}

fn print_subgraph(subgraph: &GraphSubgraph, source_node: &str) {
    let neighbors: Vec<_> = subgraph
        .nodes
        .iter()
        .filter(|n| n.id != source_node)
        .collect();

    println!(
        "\nFound {} neighbors for '{}':",
        neighbors.len(),
        source_node
    );

    for (i, node) in neighbors.iter().enumerate() {
        println!("{}: {} ({})", i + 1, node.label, node.id);
    }

    if !subgraph.edges.is_empty() {
        println!("\nEdges:");
        for edge in &subgraph.edges {
            println!("  - {} -[{}]-> {}", edge.src, edge.relation, edge.dst);
        }
    }
}
