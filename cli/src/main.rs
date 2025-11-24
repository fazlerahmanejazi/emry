mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{Cli, Commands};

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
            summary,
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
            summary,
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
        Commands::Ask {
            query,
            depth,
            agent,
            output,
            json,
            max_per_step,
            max_observations,
            max_tokens,
        } => {
            match commands::handle_ask(
                query,
                depth,
                agent,
                output,
                json,
                max_per_step,
                max_observations,
                max_tokens,
                cli.config.as_deref(),
            )
            .await
            {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Ask failed: {}", e);
                    1
                }
            }
        }
        Commands::Status => match commands::handle_status(cli.config.as_deref()) {
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
