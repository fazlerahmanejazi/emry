use std::path::Path;

pub fn print_snippet(chunk: &emry_core::models::Chunk, root: &Path, context: usize) {
    let path = root.join(&chunk.file_path);
    if let Ok(content) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = content.lines().collect();
        let start = chunk.start_line.saturating_sub(1).saturating_sub(context);
        let end = usize::min(lines.len(), chunk.end_line + context);
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_no = start + i + 1;
            let marker = if line_no >= chunk.start_line && line_no <= chunk.end_line {
                ""
            } else {
                " "
            };
            println!("{}{:5} {}", marker, line_no, line);
        }
    } else {
        println!("{}", chunk.content.trim());
    }
}
