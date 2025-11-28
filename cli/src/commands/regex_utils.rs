use anyhow::{anyhow, Result};
use globset::GlobSet;
use regex::Regex;
use std::path::{Path, PathBuf};

pub fn regex_search(
    root: &Path,
    pattern: &str,
    index_cfg: &emry_config::CoreConfig,
    use_ignore: bool,
) -> Result<Vec<(PathBuf, usize, String)>> {
    let mut results = Vec::new();
    let re = Regex::new(pattern).map_err(|e| anyhow!("Invalid regex '{}': {}", pattern, e))?;

    // Build include/exclude sets
    let include_set = build_globset(if index_cfg.include_paths.is_empty() {
        vec!["**/*".to_string()]
    } else {
        index_cfg.include_paths.clone()
    });
    let mut exclude_patterns: Vec<String> = index_cfg.exclude_paths.clone();
    exclude_patterns.extend(
        [
            "node_modules/**",
            "dist/**",
            "build/**",
            "target/**",
            ".git/**",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let exclude_set = build_globset(exclude_patterns);

    for entry in ignore::WalkBuilder::new(root)
        .hidden(!use_ignore)
        .ignore(use_ignore)
        .git_ignore(use_ignore)
        .git_exclude(use_ignore)
        .build()
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("Error walking: {}", err);
                continue;
            }
        };
        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let rel_str = rel.to_string_lossy();

        if let Some(set) = &include_set {
            if !set.is_match(rel_str.as_ref()) {
                continue;
            }
        }
        if let Some(set) = &exclude_set {
            if set.is_match(rel_str.as_ref()) {
                continue;
            }
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            for (idx, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    results.push((rel.clone(), idx + 1, line.to_string()));
                }
            }
        }
    }

    Ok(results)
}

fn build_globset(patterns: Vec<String>) -> Option<GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pat in patterns {
        if let Ok(glob) = globset::Glob::new(&pat) {
            builder.add(glob);
        } else {
            eprintln!("Ignoring invalid glob pattern: {}", pat);
        }
    }
    match builder.build() {
        Ok(set) => Some(set),
        Err(err) => {
            eprintln!("Failed to build globset: {}", err);
            None
        }
    }
}
