// File scanner for repository indexing
// File scanner for repository indexing
use crate::models::Language;
use coderet_config::CoreConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn, trace};

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub language: Language,
}

pub fn scan_repo(root: &Path, config: &CoreConfig) -> Vec<ScannedFile> {
    const DEFAULT_EXCLUDES: &[&str] = &[
        "node_modules/**",
        "dist/**",
        "build/**",
        "target/**",
        ".git/**",
    ];

    let include_set = build_globset(if config.include_paths.is_empty() {
        vec!["**/*".to_string()]
    } else {
        config.include_paths.clone()
    });

    let mut exclude_patterns: Vec<String> = config.exclude_paths.clone();
    exclude_patterns.extend(DEFAULT_EXCLUDES.iter().map(|s| s.to_string()));
    let exclude_set = build_globset(exclude_patterns);

    let mut files = Vec::new();
    let builder = WalkBuilder::new(root);
    let walker = builder.build();

    for result in walker {
        match result {
            Ok(entry) => {
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    let path = entry.path();
                    let rel_path = path.strip_prefix(root).unwrap_or(path);
                    let rel_str = rel_path.to_string_lossy();

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

                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        let lang = Language::from_extension(ext);
                        if path.to_string_lossy().contains("search.rs") {
                            trace!(
                                "Path: {}, Ext: {:?}, Lang: {:?}",
                                path.display(),
                                ext,
                                lang
                            );
                        }
                        if lang != Language::Unknown {
                            files.push(ScannedFile {
                                path: path.to_path_buf(),
                                language: lang,
                            });
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!("Error scanning path: {}", err);
            }
        }
    }

    files
}

pub fn build_globset(patterns: Vec<String>) -> Option<GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        if let Ok(glob) = Glob::new(&pat) {
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
