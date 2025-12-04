mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let exit_code = match cli.command {
        Commands::Index { full } => {
            match commands::handle_index(full, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    commands::ui::print_error(&format!("Index failed: {}", e));
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
            smart,
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
            smart,
        )
        .await
        {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Search failed: {}", e));
                1
            }
        },
        Commands::Ask { query, verbose } => {
            match commands::handle_ask(query, verbose, cli.config.as_deref()).await {
                Ok(_) => 0,
                Err(e) => {
                    commands::ui::print_error(&format!("Ask failed: {}", e));
                    1
                }
            }
        }
        Commands::Graph(args) => match commands::handle_graph(args, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Graph command failed: {}", e));
                1
            }
        },
        Commands::Status => match commands::handle_status(cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Status failed: {}", e));
                1
            }
        },
        Commands::Inspect(args) => match commands::handle_inspect(args, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Inspect failed: {}", e));
                1
            }
        },
        Commands::Cat { files } => match commands::handle_cat(files, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Cat failed: {}", e));
                1
            }
        },
        Commands::Explore { path, depth } => match commands::handle_explore(path, depth, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Explore failed: {}", e));
                1
            }
        },
        Commands::Architecture { mode, verbose } => match commands::handle_architecture(mode, verbose, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Architecture analysis failed: {}", e));
                1
            }
        },
        Commands::Impact { file, start, end, verbose } => match commands::handle_impact(file, start, end, verbose, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Impact analysis failed: {}", e));
                1
            }
        },
        Commands::Focus { topic, verbose } => match commands::handle_focus(topic, verbose, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Focus failed: {}", e));
                1
            }
        },
        Commands::Map { depth, verbose } => match commands::handle_codebase_map(depth, verbose, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Map generation failed: {}", e));
                1
            }
        },
        Commands::Debug => match commands::handle_debug(cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Debug failed: {}", e));
                1
            }
        },
        Commands::Explain { verbose } => match commands::handle_explain(verbose, cli.config.as_deref()).await {
            Ok(_) => 0,
            Err(e) => {
                commands::ui::print_error(&format!("Explain failed: {}", e));
                1
            }
        },
    };

    std::process::exit(exit_code);
}
