use crate::models::Language;
use crate::tags_extractor::TagsExtractor;
use anyhow::Result;
use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use std::path::Path;

/// Generates a high-level map of the codebase.
/// 
/// This function traverses the directory structure up to `max_depth` and
/// generates a compressed outline for each supported file.
/// 
/// It respects .gitignore files and the provided exclude patterns.
pub fn generate_codebase_map(root_path: &Path, max_depth: usize, exclude_patterns: &[String]) -> Result<String> {
    let mut map = String::new();
    let mut extractor = TagsExtractor::new()?;

    map.push_str(&format!("# Codebase Map for {}\n\n", root_path.display()));

    let mut builder = GlobSetBuilder::new();
    for pat in exclude_patterns {
        if let Ok(glob) = Glob::new(pat) {
            builder.add(glob);
        }
    }
    let exclude_set = builder.build().ok();

    let walker = WalkBuilder::new(root_path)
        .hidden(false)
        .git_ignore(true)
        .max_depth(Some(max_depth))
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                
                if path == root_path {
                    continue;
                }

                let relative_path = match path.strip_prefix(root_path) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                if let Some(set) = &exclude_set {
                    if set.is_match(relative_path) {
                        continue;
                    }
                }

                if relative_path.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
                    continue;
                }

                let depth = relative_path.components().count();
                let indent = "  ".repeat(depth.saturating_sub(1));

                if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                    map.push_str(&format!("{}- {}/\n", indent, relative_path.file_name().unwrap_or_default().to_string_lossy()));
                } else {
                    let language = Language::from_path(path);
                    if language != Language::Unknown {
                        map.push_str(&format!("{}- {}\n", indent, relative_path.file_name().unwrap_or_default().to_string_lossy()));
                        
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(symbols) = extractor.extract_symbols(&content, path, &language) {
                                let top_level_symbols: Vec<String> = symbols.into_iter()
                                    .filter(|s| s.kind == "class" || s.kind == "function" || s.kind == "interface" || s.kind == "struct")
                                    .map(|s| s.name)
                                    .take(5)
                                    .collect();
                                
                                if !top_level_symbols.is_empty() {
                                    map.push_str(&format!("{}  (Symbols: {})\n", indent, top_level_symbols.join(", ")));
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_generate_codebase_map_respects_ignores() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();

        let main_rs = root.join("main.rs");
        File::create(&main_rs)?.write_all(b"fn main() {}")?;

        std::fs::create_dir(root.join(".git"))?;

        let ignored_rs = root.join("ignored.rs");
        File::create(&ignored_rs)?.write_all(b"fn ignored() {}")?;

        let gitignore = root.join(".gitignore");
        File::create(&gitignore)?.write_all(b"ignored.rs")?;

        let config_ignored_rs = root.join("config_ignored.rs");
        File::create(&config_ignored_rs)?.write_all(b"fn config_ignored() {}")?;

        let excludes = vec!["config_ignored.rs".to_string()];
        let map = generate_codebase_map(root, 5, &excludes)?;

        assert!(map.contains("main.rs"), "Should contain main.rs");
        assert!(!map.contains("ignored.rs"), "Should NOT contain ignored.rs (gitignore)");
        assert!(!map.contains("config_ignored.rs"), "Should NOT contain config_ignored.rs (exclude pattern)");

        Ok(())
    }
}
