use console;

fn main() {
    let step = 1;
    let prefix = format!("┌── Step {} ", step);
    let total_width: usize = 80;
    let suffix_len = total_width.saturating_sub(prefix.len() + 1); // +1 for '┐'
    let suffix = "─".repeat(suffix_len);
    let header = format!("{}{}┐", prefix, suffix);
    println!("{}", header);

    let text = "Test Line";
    let width: usize = 76;
    let visual_len = console::measure_text_width(text);
    let padding = width.saturating_sub(visual_len);
    let body = format!(
        "{} {} {}{}",
        "│",
        text,
        " ".repeat(padding),
        "│"
    );
    println!("{}", body);

    let bottom = "└──────────────────────────────────────────────────────────────────────────────┘";
    println!("{}", bottom);
}
