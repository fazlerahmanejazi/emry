mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{handle_index, handle_search, handle_ask, Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Commands::Index { full, summarize } => {
            match handle_index(full, summarize, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Index failed: {}", e);
                    1
                }
            }
        }
        Commands::Search { query, mode, top, lang, path, tui, symbol, summary, paths, regex, explain, with_summaries } => {
            eprintln!("DEBUG: main calling handle_search");
            match handle_search(query, mode, top, lang, path, tui, symbol, summary, paths, regex, explain, with_summaries, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Search failed: {}", e);
                    1
                }
            }
        }
        Commands::Ask { query, top, mode, lang, path, with_summaries, show_snippets } => {
            match handle_ask(query, top, mode, lang, path, with_summaries, show_snippets, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Ask failed: {}", e);
                    1
                }
            }
        }
        Commands::Status => {
            match commands::handle_status(cli.config.as_deref()) {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Status failed: {}", e);
                    1
                }
            }
        }
        Commands::Tui => {
            match tui::run_tui().await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("TUI failed: {}", e);
                    1
                }
            }
        }
    };

    std::process::exit(exit_code);
}
