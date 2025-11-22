mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{handle_index, handle_search, Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index { full, summarize } => {
            handle_index(full, summarize).await?;
        }
        Commands::Search { query, mode, top, lang: _, path: _, tui, symbol, summary } => {
            handle_search(query, mode, top, tui, symbol, summary).await?;
        }
        Commands::Status => {
            println!("Status command not implemented yet.");
        }
        Commands::Tui => {
            tui::run_tui().await?;
        }
    }

    Ok(())
}
