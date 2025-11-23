mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{handle_ask, handle_index, handle_search, Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Commands::Index { full, summarize, summarize_levels, summarize_model, summarize_max_tokens, summarize_prompt_version } => {
            match handle_index(
                full,
                summarize,
                summarize_levels,
                summarize_model,
                summarize_max_tokens,
                summarize_prompt_version,
                cli.config.as_deref(),
            )
            .await {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Index failed: {}", e);
                    1
                }
            }
        }
        Commands::Search {
            query,
            mode,
            top,
            lang,
            path,
            tui,
            symbol,
            summary,
            paths,
            regex,
            no_ignore,
            explain,
            with_summaries,
        } => {
            eprintln!("DEBUG: main calling handle_search");
            match handle_search(
                query,
                mode,
                top,
                lang,
                path,
                tui,
                symbol,
                summary,
                paths,
                regex,
                no_ignore,
                explain,
                with_summaries,
                cli.config.as_deref(),
            )
            .await
            {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("Search failed: {}", e);
                    1
                }
            }
        }
        Commands::Ask {
            query,
            mode,
            depth,
            with_summaries,
            use_graph,
            use_symbols,
        } => {
            match handle_ask(
                query,
                mode,
                depth,
                with_summaries,
                use_graph,
                use_symbols,
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
