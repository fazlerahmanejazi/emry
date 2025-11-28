use emry_store::content_store::ContentStore;
use std::path::Path;

pub fn print_snippet(
    chunk: &emry_core::models::Chunk,
    root: &Path,
    context: usize,
    store: Option<&ContentStore>,
) {
    println!(
        "{}",
        snippet_as_string(chunk, root, context, store).trim_end()
    );
}

pub fn snippet_as_string(
    chunk: &emry_core::models::Chunk,
    root: &Path,
    context: usize,
    store: Option<&ContentStore>,
) -> String {
    if let Some(store) = store {
        if let Ok(Some(content)) = store.get(&chunk.content_hash) {
            return render_snippet(&content, chunk, context);
        }
    }

    let path = root.join(&chunk.file_path);
    if let Ok(content) = std::fs::read_to_string(&path) {
        return render_snippet(&content, chunk, context);
    }

    if let Some(store) = store {
        if let Ok(Some(content)) = store.get(&chunk.content_hash) {
            return render_snippet(&content, chunk, context);
        }
    }

    chunk.content.trim().to_string()
}

fn render_snippet(content: &str, chunk: &emry_core::models::Chunk, context: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
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
