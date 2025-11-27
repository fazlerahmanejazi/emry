use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::process::Command;
use std::time::Duration;

mod app;
use app::{AgentStatus, App, AppMode};

use coderet_config::Config;
use coderet_core::ranking::RankConfig;
use coderet_graph::graph::CodeGraph;
use coderet_index::lexical::LexicalIndex;
use coderet_pipeline::manager::IndexManager;
use coderet_index::vector::VectorIndex;
use coderet_store::chunk_store::ChunkStore;
use coderet_store::file_store::FileStore;
use coderet_store::relation_store::RelationStore;
use coderet_store::storage::Store;
use std::sync::Arc;
use tokio::sync::Mutex;

// Agent imports
use coderet_agent::agent_loop::AgentLoop;
use coderet_context::RepoContext;

pub async fn run_tui() -> Result<()> {
    // Initialize index manager once
    let root = std::env::current_dir()?;
    let branch = current_branch();
    let index_dir = root.join(".codeindex").join("branches").join(branch);
    if !index_dir.exists() {
        return Err(anyhow::anyhow!(
            "Index not found. Run `coderet index` first."
        ));
    }

    let store = Store::open(&index_dir.join("store.db"))?;
    let file_store = Arc::new(FileStore::new(store.clone())?);
    let chunk_store = Arc::new(ChunkStore::new(store.clone())?);
    let relation_store = Arc::new(RelationStore::new(store.clone())?);
    let graph = Arc::new(CodeGraph::new(store.clone())?);
    let content_store = Arc::new(coderet_store::content_store::ContentStore::new(store.clone())?);
    let file_blob_store = Arc::new(coderet_store::file_blob_store::FileBlobStore::new(
        store.clone(),
    )?);

    let lexical = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
    let vector = Arc::new(Mutex::new(
        VectorIndex::new(&index_dir.join("vector.lance")).await?,
    ));

    let manager = Arc::new(IndexManager::new(
        lexical,
        vector,
        None,
        file_store,
        chunk_store,
        content_store.clone(),
        file_blob_store,
        relation_store,
        graph.clone(),
        None,
    ));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<String>>();

    loop {
        terminal.draw(|f| draw_ui(f, &app))?;

        // Check for search results
        if let Ok(res) = rx.try_recv() {
            match res {
                Ok(text) => app.on_result(text),
                Err(e) => app.on_error(e.to_string()),
            }
        }

        if let Some(ev) = App::poll_event(Duration::from_millis(100))? {
            if let crossterm::event::Event::Key(key) = ev {
                if let Some(cmd) = app.on_key(key.code) {
                    if cmd == "__quit__" {
                        break;
                    }
                    // Spawn task based on mode
                    let tx = tx.clone();
                    let query = cmd.clone();
                    match app.mode {
                        AppMode::Search => {
                            let manager = manager.clone();
                            let store = content_store.clone();
                            tokio::spawn(async move {
                                let res = handle_query(&query, manager, store).await;
                                let _ = tx.send(res);
                            });
                        }
                        AppMode::Agent => {
                            tokio::spawn(async move {
                                let res = handle_agent_query(&query).await;
                                let _ = tx.send(res);
                            });
                        }
                    }
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

async fn handle_query(
    query: &str,
    manager: Arc<IndexManager>,
    content_store: Arc<coderet_store::content_store::ContentStore>,
) -> Result<String> {
    let root = std::env::current_dir()?;
    let config = Config::load().unwrap_or_default();
    let results = manager
        .search_ranked(query, config.search.top_k, Some(rank_cfg(&config)))
        .await?;

    if results.is_empty() {
        return Ok("No results.".to_string());
    }

    let mut out = String::new();
    for (i, hit) in results.iter().enumerate() {
        let chunk = &hit.chunk;
        out.push_str(&format!(
            "\n#{} [{:.3}] lex={:.3?} vec={:.3?} graph={:.3?} sym={:.3?} {}:{}-{}",
            i + 1,
            hit.score,
            hit.lexical_score,
            hit.vector_score,
            hit.graph_boost,
            hit.symbol_boost,
            chunk.file_path.display(),
            chunk.start_line,
            chunk.end_line
        ));
        if let Some(path) = &hit.graph_path {
            out.push_str(&format!("\npath: {}", path.join(" | ")));
        }
        out.push_str(&format!(
            "\n{}\n",
            build_snippet(&root, chunk, 2, Some(content_store.as_ref()))
        ));
        if i >= 2 {
            break;
        }
    }
    Ok(out)
}

async fn handle_agent_query(query: &str) -> Result<String> {
    // Initialize agent
    let ctx = Arc::new(RepoContext::from_env(None).await?);
    
    let index_dir = ctx.index_dir.clone();
    let manager = Arc::new(IndexManager::new(
        Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?),
        Arc::new(Mutex::new(VectorIndex::new(&index_dir.join("vector.lance")).await?)),
        ctx.embedder.clone(),
        ctx.file_store.clone(),
        ctx.chunk_store.clone(),
        ctx.content_store.clone(),
        ctx.file_blob_store.clone(),
        ctx.relation_store.clone(),
        ctx.graph.clone(),
        Some(ctx.summary_index.clone()),
    ));

    let agent = AgentLoop::new(ctx.clone(), manager)?;

    // Execute agent query
    let result = agent.run(query, false).await?;

    // Format result
    let mut out = String::new();
    out.push_str(&format!("\n=== Agent Answer ===\n{}\n", result.final_answer));

    out.push_str("\n=== Trace ===\n");
    for (i, turn) in result.turns.iter().enumerate() {
        out.push_str(&format!("{}. Thought: {}\n", i + 1, turn.thought));
        if let Some(tool) = &turn.tool_call {
            out.push_str(&format!("   Action: {} {:?}\n", tool.tool_name, tool.args));
        }
        if let Some(obs) = &turn.observation {
            let snippet = if obs.len() > 100 {
                format!("{}\
...", &obs[..100])
            } else {
                obs.clone()
            };
            out.push_str(&format!("   Observation: {}\n", snippet));
        }
    }

    Ok(out)
}

fn rank_cfg(config: &Config) -> RankConfig {
    RankConfig {
        lexical_weight: config.ranking.lexical,
        vector_weight: config.ranking.vector,
        graph_weight: config.ranking.graph,
        symbol_weight: config.ranking.symbol,
        graph_max_depth: config.graph.max_depth,
        graph_decay: config.graph.decay,
        graph_path_weight: config.graph.path_weight,
        bm25_k1: config.bm25.k1,
        bm25_b: config.bm25.b,
        bm25_avg_len: config.bm25.avg_len,
        edge_weights: config.graph.edge_weights.clone(),
        summary_similarity_threshold: 0.25,
        summary_boost_weight: config.ranking.summary,
    }
}

fn draw_ui(f: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default() 
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(f.area());

    let mode_indicator = match app.mode {
        AppMode::Search => " [Search]",
        AppMode::Agent => " [Agent]",
    };
    let input = Paragraph::new(app.input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Query{}", mode_indicator)),
    );
    f.render_widget(input, chunks[0]);

    let mut messages: Vec<Line> = app
        .messages
        .iter()
        .rev()
        .take(20)
        .map(|m| {
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", m.role),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(m.content.clone()),
            ])
        })
        .collect();

    // Add agent status info in agent mode
    if app.mode == AppMode::Agent && app.agent_status != AgentStatus::Idle {
        let status_color = match app.agent_status {
            AgentStatus::Planning => Color::Cyan,
            AgentStatus::Executing => Color::Yellow,
            AgentStatus::Synthesizing => Color::Magenta,
            AgentStatus::Done => Color::Green,
            AgentStatus::Error => Color::Red,
            AgentStatus::Idle => Color::Gray,
        };
        messages.insert(
            0,
            Line::from(vec![Span::styled(
                "─".repeat(50),
                Style::default().fg(Color::DarkGray),
            )]),
        );
        if let Some(plan) = &app.agent_plan {
            messages.insert(
                0,
                Line::from(vec![ 
                    Span::styled(
                        "Plan: ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(plan.chars().take(100).collect::<String>()),
                ]),
            );
        }
        if !app.agent_observations.is_empty() {
            messages.insert(
                0,
                Line::from(vec![ 
                    Span::styled(
                        format!("Observations: {} ", app.agent_observations.len()),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(
                        app.agent_observations
                            .last()
                            .map(|o| o.chars().take(80).collect::<String>())
                            .unwrap_or_default(),
                    ),
                ]),
            );
        }
        messages.insert(
            0,
            Line::from(vec![Span::styled(
                format!("Status: {:?} ", app.agent_status),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )]),
        );
    }

    let output = Paragraph::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Messages"))
        .scroll((0, 0));
    f.render_widget(output, chunks[1]);

    let status = Paragraph::new(app.status.as_str())
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status, chunks[2]);
}

fn build_snippet(
    root: &std::path::Path,
    chunk: &coderet_core::models::Chunk,
    context: usize,
    store: Option<&coderet_store::content_store::ContentStore>,
) -> String {
    if let Some(store) = store {
        if let Ok(Some(text)) = store.get(&chunk.content_hash) {
            return render_snippet(&text, chunk, context);
        }
    }

    let path = root.join(&chunk.file_path);
    if let Ok(text) = std::fs::read_to_string(&path) {
        return render_snippet(&text, chunk, context);
    }

    if let Some(store) = store {
        if let Ok(Some(text)) = store.get(&chunk.content_hash) {
            return render_snippet(&text, chunk, context);
        }
    }

    chunk
        .content
        .lines()
        .take(context * 2 + 2)
        .collect::<Vec<_>>()
        .join("\n")
}

fn current_branch() -> String {
    if let Ok(out) = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
    {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let trimmed = s.trim();
                if !trimmed.is_empty() && trimmed != "HEAD" {
                    return trimmed.to_string();
                }
            }
        }
    }
    "default".to_string()
}

fn render_snippet(text: &str, chunk: &coderet_core::models::Chunk, context: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = chunk.start_line.saturating_sub(1).saturating_sub(context);
    let end = usize::min(lines.len(), chunk.end_line + context);
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_no = start + i + 1;
            let marker = if line_no >= chunk.start_line && line_no <= chunk.end_line {
                ">"
            } else {
                " "
            };
            format!("{}{:5} {}", marker, line_no, line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}