use anyhow::Result;
use clap::ValueEnum;
use emry_agent::project as agent_context;
use emry_core::models::Language;
use emry_engine::search::service::SearchService;
use std::path::Path;
use std::path::PathBuf;

use super::regex_utils;
use super::utils::{build_single_globset, path_matches};
use emry_agent::ops::rewriter::QueryRewriter;
use emry_agent::llm::OpenAIProvider;

use super::ui;
use console::Style;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum CliSearchMode {
    Lexical,
    Semantic,
    Hybrid,
}

pub async fn handle_search(
    query: String,
    config_path: Option<&Path>,
    limit: usize,
    _mode: Option<CliSearchMode>,
    lang: Option<String>,
    path: Option<String>,
    symbol: bool,
    regex: bool,
    no_ignore: bool,
    smart: bool,
) -> Result<()> {
    ui::print_header(&format!("Searching for: {}{}", query, if smart { " (Smart)" } else { "" }));

    let ctx = agent_context::RepoContext::from_env(config_path).await?;


    let embedder = ctx.embedder.clone();
    
    let surreal_store = ctx.surreal_store.clone()
        .ok_or_else(|| anyhow::anyhow!("SurrealStore not initialized in context"))?;
    let search_service = SearchService::new(surreal_store.clone(), embedder.clone());
    
    if symbol {
        return handle_symbol_search(&query, &ctx, limit, lang, path).await;
    }

    if regex {
        return handle_regex_search(&query, &ctx, lang, path, no_ignore);
    }

    handle_smart_search(&query, &ctx, &search_service, limit, smart).await?;

    Ok(())
}

async fn handle_symbol_search(
    query: &str,
    ctx: &agent_context::RepoContext,
    _limit: usize,
    lang: Option<String>,
    path: Option<String>,
) -> Result<()> {
    let root = &ctx.root;
    let matcher = build_single_globset(path.as_deref());
    let lang_filter = lang.as_deref().map(Language::from_name);
    let mut matches = Vec::new();

    if let Some(store) = &ctx.surreal_store {
        if let Ok(nodes) = store.find_nodes_by_label(query, None).await {
            for node in nodes {
                let file_path = PathBuf::from(&node.file_path);
                if let Some(lf) = lang_filter.as_ref() {
                    if let Some(ext) = file_path.extension().and_then(|s| s.to_str()) {
                        if Language::from_extension(ext) != *lf {
                            continue;
                        }
                    }
                }
                if !path_matches(&matcher, root, &file_path) {
                    continue;
                }
                matches.push((node.label.clone(), file_path, node.id.clone()));
            }
        }
    }

    if matches.is_empty() {
         println!("No symbol matches found.");
    } else {
        println!("Found {} symbol matches:", matches.len());
        for (i, (name, file_path, id)) in matches.iter().enumerate() {
             println!(
                "{} {} ({})",
                Style::new().dim().apply_to(format!("{}.", i + 1)),
                Style::new().bold().cyan().apply_to(name),
                Style::new().dim().apply_to(file_path.display())
            );
            println!("   {}", Style::new().dim().apply_to(format!("ID: {}", id)));
        }
    }
    Ok(())
}

fn handle_regex_search(
    query: &str,
    ctx: &agent_context::RepoContext,
    lang: Option<String>,
    path: Option<String>,
    no_ignore: bool,
) -> Result<()> {
    let root = &ctx.root;
    let config = &ctx.config;
    let matcher = build_single_globset(path.as_deref());
    let lang_filter = lang.as_deref().map(Language::from_name);
    
    let matches = regex_utils::regex_search(root, query, &config.core, !no_ignore)?;
    
    if matches.is_empty() {
        println!("No matches for regex '{}'.", query);
    } else {
        println!("Regex matches for '{}':", query);
        for (p, line, content) in matches {
            if let Some(lf) = lang_filter.as_ref() {
                if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                    if Language::from_extension(ext) != *lf {
                        continue;
                    }
                }
            }
            if !path_matches(&matcher, root, &p) {
                continue;
            }
            let rel = p.strip_prefix(root).unwrap_or(&p);
            ui::print_search_match(0, &rel.to_string_lossy(), line, line, &content);
        }
    }
    Ok(())
}

async fn handle_smart_search(
    query: &str,
    _ctx: &agent_context::RepoContext,
    search_service: &SearchService,
    limit: usize,
    smart: bool,
) -> Result<()> {
    if smart {
        let keywords = if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            if let Ok(llm) = OpenAIProvider::new(model, api_key, 60) {
                let rewriter = QueryRewriter::new(llm);
                match rewriter.rewrite(query).await {
                    Ok(expanded) => {
                        ui::print_panel("Query", &format!("Original: {}\nKeywords: {:?}\nIntent: {}", query, expanded.keywords, expanded.intent), Style::new().green(), None);
                        Some(expanded.keywords)
                    },
                    Err(_) => None
                }
            } else {
                None
            }
        } else {
            ui::print_panel("Warning", "OPENAI_API_KEY not set. Skipping query expansion.", Style::new().yellow(), None);
            None
        };

        let context_graph = search_service.search_with_context(query, limit, keywords.as_deref()).await?;
        let grouped = context_graph.group_by_symbol();
        
        if grouped.groups.is_empty() && grouped.unassigned.is_empty() {
            println!("No smart matches found.");
        } else {
            println!("Found {} symbol groups and {} unassigned matches:", grouped.groups.len(), grouped.unassigned.len());
            
            let mut match_index = 0;

            for group in grouped.groups {
                let start_line = group.anchors.iter().map(|c| c.chunk.start_line).min().unwrap_or(0);
                let end_line = group.anchors.iter().map(|c| c.chunk.end_line).max().unwrap_or(0);
                let content = emry_core::models::ScoredChunk::concatenate_chunks(&group.anchors);

                match_index += 1;
                println!("{} {} {} {}", 
                    Style::new().bold().blue().apply_to(format!("#{}", match_index)),
                    Style::new().dim().apply_to("Symbol:"),
                    Style::new().bold().cyan().apply_to(&group.symbol.name),
                    Style::new().dim().apply_to(format!("({}:{}-{})", group.symbol.file_path.display(), start_line, end_line))
                );
                
                if !group.calls.is_empty() {
                    print!("  {} Calls: ", Style::new().dim().apply_to("↳"));
                    for (j, call) in group.calls.iter().enumerate() {
                        if j > 0 { print!(", "); }
                        print!("{}", Style::new().yellow().apply_to(&call.name));
                    }
                    println!();
                }

                println!("{}", Style::new().dim().apply_to(content.trim()));
                println!();
            }

            if !grouped.unassigned.is_empty() {
                println!("Other Matches:");
                for anchor in grouped.unassigned {
                    match_index += 1;
                    ui::print_search_match(
                        match_index,
                        &anchor.chunk.file_path.display().to_string(),
                        anchor.chunk.start_line,
                        anchor.chunk.end_line,
                        &anchor.chunk.content
                    );
                }
            }
        }
    } else {
        let results = search_service.search(query, limit, None).await?;
        
        if results.is_empty() {
            println!("No semantic matches found.");
        } else {
            println!("Found {} semantic matches:", results.len());
            for (i, chunk) in results.iter().enumerate() {
                let file_id = chunk.file.id.to_string();
                let path = file_id.strip_prefix("file:").unwrap_or(&file_id);
                ui::print_search_match(
                    i + 1,
                    path,
                    chunk.start_line,
                    chunk.end_line,
                    &chunk.content
                );
            }
        }
    }

    Ok(())
}