
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;
use std::process::Command;
use termimad::{FmtText, MadSkin};

pub fn current_branch() -> String {
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

pub fn render_markdown_answer(text: &str) -> String {
    let skin = MadSkin::default();
    let (w, _) = termimad::terminal_size();
    let width = std::cmp::max(20, w.saturating_sub(4) as usize);
    FmtText::from(&skin, text, Some(width)).to_string()
}

pub fn build_single_globset(pattern: Option<&str>) -> Option<GlobSet> {
    let pat = pattern?;
    let mut builder = GlobSetBuilder::new();
    if let Ok(glob) = Glob::new(pat) {
        builder.add(glob);
    } else {
        eprintln!("Invalid glob pattern '{}', ignoring.", pat);
        return None;
    }
    match builder.build() {
        Ok(set) => Some(set),
        Err(e) => {
            eprintln!("Failed to build globset: {}", e);
            None
        }
    }
}

pub fn path_matches(matcher: &Option<GlobSet>, root: &Path, path: &Path) -> bool {
    if let Some(set) = matcher {
        let rel = path.strip_prefix(root).unwrap_or(path);
        set.is_match(rel.to_string_lossy().as_ref())
    } else {
        true
    }
}
