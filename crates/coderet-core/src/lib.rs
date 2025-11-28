pub mod chunking;
pub mod llm;
pub mod models;
pub mod ranking;
pub mod relations;
pub mod scanner;
pub mod summaries;
pub mod symbols;
pub mod tags_extractor;
pub mod traits;
pub mod stack_graphs;
pub mod stack_graphs_symbols;

// Re-export commonly used stack-graphs functions for convenience
pub use stack_graphs_symbols::{
    extract_call_edges_stack_graphs,
    extract_symbols_stack_graphs,
    supports_stack_graphs,
};

#[cfg(test)]
mod relations_test;
