use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
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
                Constraint::Min(1),    // Chat history
                Constraint::Length(1), // Status bar
            ]
            .as_ref(),
        )
        .split(f.area());

    let msg = match app.input_mode {
        InputMode::Normal => "Normal Mode - Press 'a' to ask, 'q' to quit, 'j'/'k' to scroll.",
        InputMode::Editing => "Editing Mode - Press Esc to cancel, Enter to send.",
    };

    let help_message = Paragraph::new(msg).style(Style::default().fg(Color::Yellow));
    f.render_widget(help_message, chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Ask Agent"));
    f.render_widget(input, chunks[1]);

    // Render chat history
    let mut lines = Vec::new();
    for msg in &app.messages {
        let style = match msg.role.as_str() {
            "user" => Style::default().fg(Color::Cyan),
            "agent" => Style::default().fg(Color::Green),
            "system" => Style::default().fg(Color::Gray),
            _ => Style::default(),
        };
        
        let prefix = match msg.role.as_str() {
            "user" => "You: ",
            "agent" => "Agent: ",
            "system" => "[System] ",
            _ => "",
        };
        
        // Split long messages into multiple lines
        for line in msg.content.lines() {
            if lines.is_empty() || msg.role != "agent" {
                lines.push(Line::from(vec![
                    Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                    Span::styled(line, style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("       ", style),
                    Span::styled(line, style),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

    // Apply scroll offset
    let visible_lines: Vec<Line> = if app.scroll_offset < lines.len() {
        lines.into_iter().skip(app.scroll_offset).collect()
    } else {
        lines
    };

    let chat = Paragraph::new(visible_lines)
        .block(Block::default().borders(Borders::ALL).title("Chat"))
        .wrap(Wrap { trim: false });
    f.render_widget(chat, chunks[2]);

    // Status bar
    let status = Paragraph::new(app.status_message.as_str())
        .style(Style::default().bg(Color::Blue).fg(Color::White));
    f.render_widget(status, chunks[3]);
}
