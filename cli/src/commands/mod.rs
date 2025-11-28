pub mod ask;
pub mod graph;
pub mod index;
pub mod print_snippet;
pub mod regex_utils;
pub mod search;
pub mod status;
pub mod utils;

pub use ask::handle_ask;
pub use graph::{handle_graph, GraphArgs};
pub use index::handle_index;
pub use search::{handle_search, CliSearchMode};
pub use status::handle_status;


use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "emry")]
#[command(about = "tool designed for deep semantic and structural code exploration")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Index the current repository
    Index {
        /// Force a full rebuild
        #[arg(long)]
        full: bool,
    },
    /// Search the index
    Search {
        /// The query string
        query: String,

        /// Number of results
        #[arg(long, default_value_t = 10)]
        top: usize,

        /// Search mode
        #[arg(long, value_enum)]
        mode: Option<CliSearchMode>,

        /// Filter by language
        #[arg(long)]
        lang: Option<String>,

        /// Filter by path glob
        #[arg(long)]
        path: Option<String>,

        /// Search for symbol definitions (name match)
        #[arg(long)]
        symbol: bool,

        /// Treat query as regex (lexical/grep-style)
        #[arg(long)]
        regex: bool,

        /// Do not apply ignore rules (gitignore/config) for regex/grep search
        #[arg(long, default_value_t = false)]
        no_ignore: bool,
    },
    /// Ask about codebase in natural language
    Ask {
        /// The question
        query: String,
        /// Show verbose output (thoughts, tool calls, observations)
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Query the code graph directly
    Graph(GraphArgs),
    /// Show status (not yet implemented)
    Status,
}
