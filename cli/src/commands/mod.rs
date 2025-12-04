pub mod ask;

pub mod cat;
pub mod explore;
pub mod graph;
pub mod index;
pub mod inspect;
pub mod regex_utils;
pub mod search;
pub mod status;
pub mod ui;
pub mod utils;
pub mod architecture;
pub mod impact;
pub mod focus;
pub mod map;
pub mod debug;
pub mod explain;

pub use ask::handle_ask;
pub use cat::handle_cat;
pub use explore::handle_explore;
pub use graph::{handle_graph, GraphArgs};
pub use index::handle_index;
pub use inspect::{handle_inspect, InspectArgs};
pub use search::{handle_search, CliSearchMode};
pub use status::handle_status;
pub use architecture::handle_architecture;
pub use impact::handle_impact;
pub use focus::handle_focus;
pub use map::handle_codebase_map;
pub use debug::handle_debug;
pub use explain::handle_explain;


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

        /// Enable smart search (Query Rewriting + Subgraph Retrieval)
        #[arg(long, default_value_t = false)]
        smart: bool,
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
    /// Inspect a node by ID
    Inspect(InspectArgs),
    /// Batch read files
    Cat {
        /// Files to read
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Explore a module/directory
    Explore {
        /// Path to explore
        path: String,
        /// Depth of exploration
        #[arg(long, default_value_t = 1)]
        depth: usize,
    },
    /// Analyze codebase architecture
    Architecture {
        /// Analysis mode: 'fast' or 'deep'
        #[arg(long, default_value = "fast")]
        mode: String,
        /// Show verbose output (progress steps)
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Analyze impact of changes
    Impact {
        /// File path
        file: PathBuf,
        /// Start line
        start: usize,
        /// End line
        end: usize,
        /// Show verbose output
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Smart Focus (Auto-Context)
    Focus {
        /// Topic to focus on
        topic: String,
        /// Show verbose output
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Generate a high-level map of the codebase
    Map {
        /// Depth of traversal
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// Show verbose output
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Debug database stats
    Debug,
    /// Explain the project functionality and capabilities
    Explain {
        /// Show verbose output
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
}
