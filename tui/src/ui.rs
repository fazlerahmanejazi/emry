use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints(
            [
                Constraint::Length(1), // Help message
                Constraint::Length(3), // Input
                Constraint::Min(1),    // Results
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
    
    // Render results
    let items: Vec<ListItem> = app
        .results
        .iter()
        .map(|(score, chunk)| {
            let content_preview = chunk.content.lines().next().unwrap_or("").to_string();
            let path = chunk.file_path.to_string_lossy();
            let line = format!("{} (Score: {:.4}) - {}:{} - {}", 
                chunk.id, score, path, chunk.start_line, content_preview);
            ListItem::new(line)
        })
        .collect();

    let results = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        .highlight_symbol("> ");
        
    f.render_widget(results, chunks[2]);
    
    // Status bar
    let status = Paragraph::new(app.status_message.as_str())
        .style(Style::default().bg(Color::Blue).fg(Color::White));
    f.render_widget(status, chunks[3]);
}
