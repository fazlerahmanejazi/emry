use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap, ListState},
    Frame,
};
use std::path::PathBuf;
use std::fs;

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints(
            [
                Constraint::Length(1), // Help message
                Constraint::Length(3), // Input
                Constraint::Min(1),    // Results and detail
                Constraint::Length(1), // Status bar
            ]
            .as_ref(),
        )
        .split(f.area());

    let msg = match app.input_mode {
        InputMode::Normal => "Normal Mode - Press 's' to search, 'q' to quit.",
        InputMode::Editing => "Editing Mode - Press Esc to stop editing, Enter to search.",
    };
    
    let help_message = Paragraph::new(msg).style(Style::default().fg(Color::Yellow));
    f.render_widget(help_message, chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Search Query"));
    f.render_widget(input, chunks[1]);
    
    // Render results + detail
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)].as_ref())
        .split(chunks[2]);

    let items: Vec<ListItem> = app
        .results
        .iter()
        .map(|res| {
            let chunk = &res.chunk;
            let content_preview = chunk.content.lines().next().unwrap_or("").to_string();
            let path = chunk.file_path.to_string_lossy();
            let line = format!("{} (Score: {:.4}) - {}:{} - {}", 
                chunk.id, res.score, path, chunk.start_line, content_preview);
            ListItem::new(line)
        })
        .collect();

    let results = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        .highlight_symbol("> ");
    let mut list_state = ListState::default();
    if let Some(idx) = app.selected_result_index {
        list_state.select(Some(idx));
    }

    f.render_stateful_widget(results, body_chunks[0], &mut list_state);

    let detail_block = Block::default().borders(Borders::ALL).title("Detail");
    if let Some(idx) = app.selected_result_index {
        if let Some(res) = app.results.get(idx) {
            let chunk = &res.chunk;
            let mut lines = Vec::new();
            lines.push(Span::raw(format!("File: {}:{}-{}", chunk.file_path.display(), chunk.start_line, chunk.end_line)));
            lines.push(Span::raw(format!(
                "Lex raw/norm/weight: {:.4} / {:.4} / {:.2}",
                res.lexical_score_raw, res.lexical_score_norm, res.lexical_weight
            )));
            lines.push(Span::raw(format!(
                "Sem raw/norm/weight: {:.4} / {:.4} / {:.2}",
                res.semantic_score_raw, res.semantic_score_norm, res.semantic_weight
            )));
            lines.push(Span::raw(format!("Final score: {:.4}", res.score)));
            lines.push(Span::raw(" "));
            let snippet = build_snippet(chunk, 4);
            for line in snippet {
                lines.push(Span::raw(line));
            }
            let detail = Paragraph::new(Text::from(Line::from(lines)))
                .wrap(Wrap { trim: false });
            f.render_widget(detail.block(detail_block), body_chunks[1]);
        } else {
            f.render_widget(detail_block, body_chunks[1]);
        }
    } else {
        f.render_widget(detail_block, body_chunks[1]);
    }
    
    // Status bar
    let status = Paragraph::new(app.status_message.as_str())
        .style(Style::default().bg(Color::Blue).fg(Color::White));
    f.render_widget(status, chunks[3]);
}

fn build_snippet(chunk: &coderet_core::models::Chunk, context: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut path = PathBuf::from(&chunk.file_path);
    if path.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            path = cwd.join(path);
        }
    }
    if let Ok(content) = fs::read_to_string(&path) {
        let lines: Vec<&str> = content.lines().collect();
        let start = chunk.start_line.saturating_sub(1).saturating_sub(context);
        let end = usize::min(lines.len(), chunk.end_line + context);
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_no = start + i + 1;
            let marker = if line_no >= chunk.start_line && line_no <= chunk.end_line { ">" } else { " " };
            out.push(format!("{}{:5} {}", marker, line_no, line));
        }
    } else {
        out.push(chunk.content.clone());
    }
    out
}
