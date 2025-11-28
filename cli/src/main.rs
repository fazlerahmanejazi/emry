mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{Cli, Commands}; // Added GraphArgs
use tui; // Added import

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let exit_code = match cli.command {
        Commands::Index { full } => {
            match commands::handle_index(full, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Index failed: {}", e);
                    1
                }
            }
        }
        Commands::Search {
            query,
            top,
            mode,
            lang,
            path,
            symbol,

            regex,
            no_ignore,
        } => match commands::handle_search(
            query,
            cli.config.as_deref(),
            top,
            mode,
            lang,
            path,
            symbol,

            regex,
            no_ignore,
        )
        .await
        {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Search failed: {}", e);
                1
            }
        },
        Commands::Ask { query, verbose } => {
            match commands::handle_ask(query, verbose, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Ask failed: {}", e);
                    1
                }
            }
        }
        Commands::Graph(args) => match commands::handle_graph(args, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Graph command failed: {}", e);
                1
            }
        },
        Commands::Status => match commands::handle_status(cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Status failed: {}", e);
                1
            }
        },
        Commands::Tui => match tui::run_tui().await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("TUI failed: {}", e);
                1
            }
        },
    };

    std::process::exit(exit_code);
}
