mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{handle_index, handle_search, Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index { full } => {
            handle_index(full).await?;
        }
        Commands::Search { query, mode, top, lang: _, path: _, tui } => {
            handle_search(query, mode, top, tui).await?;
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
