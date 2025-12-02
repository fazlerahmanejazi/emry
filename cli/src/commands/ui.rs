use console::Style;

pub fn print_header(title: &str) {
    println!("\n{}", Style::new().bold().cyan().apply_to(title));
    println!("{}", Style::new().dim().apply_to("─".repeat(title.len())));
}

pub fn print_success(msg: &str) {
    println!("{} {}", Style::new().green().bold().apply_to("SUCCESS:"), msg);
}

pub fn print_error(msg: &str) {
    eprintln!("{} {}", Style::new().red().bold().apply_to("ERROR:"), msg);
}

pub fn print_panel(title: &str, content: &str, border_color: Style, content_style: Option<Style>) {
    let width: usize = 80;
    let title_len = title.len();
    let padding = width.saturating_sub(title_len + 5);
    
    println!(
        "{} {} {}",
        border_color.apply_to("┌─"),
        Style::new().bold().apply_to(title),
        border_color.apply_to(format!("{}┐", "─".repeat(padding)))
    );

    let content_style = content_style.unwrap_or_else(Style::new);

    for line in content.lines() {
        let wrapped_lines = textwrap::wrap(line, width - 4);
        
        for wrapped_line in wrapped_lines {
            let display_line = wrapped_line.to_string();
            let display_width = console::measure_text_width(&display_line);
            let space = (width - 4).saturating_sub(display_width);
            
            println!(
                "{} {} {}{}",
                border_color.apply_to("│"),
                content_style.apply_to(display_line),
                " ".repeat(space),
                border_color.apply_to("│")
            );
        }
    }
    println!("{}", border_color.apply_to(format!("└{}┘", "─".repeat(width - 2))));
}

pub fn print_search_match(i: usize, file: &str, start_line: usize, end_line: usize, content: &str) {
    let header = if start_line == end_line {
        format!("#{} {}:{}", i, file, start_line)
    } else {
        format!("#{} {}:{}-{}", i, file, start_line, end_line)
    };
    println!("{}", Style::new().bold().blue().apply_to(header));
    println!("{}", Style::new().dim().apply_to(content.trim()));
    println!();
}

pub fn print_key_value(key: &str, value: &str) {
    println!(
        "{}: {}",
        Style::new().dim().apply_to(key),
        Style::new().bold().apply_to(value)
    );
}
