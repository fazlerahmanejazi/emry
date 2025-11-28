use anyhow::{Context, Result};
use emry_agent::cortex::context::AgentContext;
use emry_agent::cortex::Cortex;
use emry_agent::cortex::tools::{
    fs::{ListFilesTool, ReadFileTool},
    graph::InspectGraphTool,
    search::SearchCodeTool,
};
use emry_agent::llm::OpenAIProvider;
use emry_config::AgentConfig;
use emry_context as agent_context;
use emry_pipeline::manager::IndexManager;
use emry_tools::fs::FsTool;
use emry_tools::graph::GraphTool;
use emry_tools::search::Search;
use std::path::Path;
use std::sync::Arc;

use super::utils::render_markdown_answer;

pub async fn handle_ask(query: String, verbose: bool, config_path: Option<&Path>) -> Result<()> {
    if verbose {
        println!("{}", console::style(format!("Query: {}", query)).bold().magenta());
    }

    let ctx = Arc::new(agent_context::RepoContext::from_env(config_path).await?);
    let manager = Arc::new(IndexManager::new(
        ctx.lexical.clone(),
        ctx.vector.clone(),
        ctx.embedder.clone(),
        ctx.file_store.clone(),
        ctx.chunk_store.clone(),
        ctx.content_store.clone(),
        ctx.file_blob_store.clone(),
        ctx.graph.clone(),
    ));

    // Initialize AgentContext
    let mut agent_ctx = AgentContext::new(
        ctx.clone(),
        manager.clone(),
        AgentConfig::default(),
    );

    // Initialize tools
    let search_impl = Arc::new(Search::new(ctx.clone(), manager.clone()));
    let search_tool = SearchCodeTool::new(search_impl);

    let graph_impl = Arc::new(GraphTool::new(ctx.clone()));
    let graph_tool = InspectGraphTool::new(graph_impl, ctx.clone());

    let fs_impl = Arc::new(FsTool::new(ctx.clone()));
    let fs_tool = ReadFileTool::new(fs_impl.clone());
    let list_files_tool = ListFilesTool::new(fs_impl.clone());

    agent_ctx.register_tool(Arc::new(search_tool));
    agent_ctx.register_tool(Arc::new(graph_tool));
    agent_ctx.register_tool(Arc::new(fs_tool));
    agent_ctx.register_tool(Arc::new(list_files_tool));

    // Initialize LLM
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY environment variable not set")?;
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let llm = OpenAIProvider::new(model, api_key, 60)?;

    let mut cortex = Cortex::new(agent_ctx, llm);

    let answer = cortex.run(&query, |event| {
        if verbose {
            match event {
                emry_agent::cortex::CortexEvent::StepStart(step) => {
                    if step > 1 {
                        print_bottom_border();
                    }
                    print_step_header(step);
                }
                emry_agent::cortex::CortexEvent::Thought(thought) => {
                    let skin = termimad::MadSkin::default();
                    print_boxed_line("Thought:", console::Style::new().bold().green());
                    let fmt_text = skin.text(&thought, Some(74)).to_string();
                    for line in fmt_text.lines() {
                        print_aligned_line(line, console::Style::new());
                    }
                    print_aligned_line("", console::Style::new());
                }
                emry_agent::cortex::CortexEvent::ToolCall { name, args } => {
                    print_boxed_line("Tool Call:", console::Style::new().bold().yellow());
                    print_boxed_line(&format!("{}({})", name, args), console::Style::new().bold());
                    print_aligned_line("", console::Style::new());
                }
                emry_agent::cortex::CortexEvent::ToolResult { name: _, result } => {
                     print_boxed_line("Observation:", console::Style::new().bold().blue());
                     let truncated = if result.len() > 300 {
                         format!("{}...", &result[..300])
                     } else {
                         result
                     };
                     for line in truncated.lines() {
                        print_boxed_line(line, console::Style::new());
                     }
                }
            }
        }
    }).await?;
    
    if verbose {
        print_bottom_border();
    }

    println!("\n{}\n{}", console::style("Final Answer:").bold().magenta(), render_markdown_answer(&answer));

    Ok(())
}

fn print_step_header(step: usize) {
    let prefix = format!("┌── Step {} ", step);
    let total_width: usize = 86;
    let suffix_len = total_width.saturating_sub(prefix.len() + 1);
    let suffix = "─".repeat(suffix_len);
    println!("{}", console::style(format!("{}{}┐", prefix, suffix)).dim());
}

fn print_bottom_border() {
    let total_width: usize = 80;
    let suffix_len = total_width.saturating_sub(2);
    let suffix = "─".repeat(suffix_len);
    println!("{}", console::style(format!("└{}┘", suffix)).dim());
}

fn print_boxed_line(text: &str, style: console::Style) {
    let width = 76;
    let wrapped = textwrap::wrap(text, width);
    for line in wrapped {
        print_aligned_line(&line, style.clone());
    }
}

fn print_aligned_line(text: &str, style: console::Style) {
    let width: usize = 76;
    let visual_len = console::measure_text_width(text);
    let padding = width.saturating_sub(visual_len);
    println!(
        "{} {} {}{}",
        console::style("│").dim(),
        style.apply_to(text),
        " ".repeat(padding),
        console::style("│").dim()
    );
}
