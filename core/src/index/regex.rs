use crate::config::IndexConfig;
use anyhow::Result;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use crate::scanner::build_globset;

pub struct RegexSearcher;

impl RegexSearcher {
    pub fn search(
        root: &Path,
        pattern: &str,
        _config: &IndexConfig,
        // We need a way to map back to chunks.
        // For now, we'll return (path, line, content) and let the caller map to chunks
        // or we can pass a closure/interface to query the index.
        // But to keep it simple, let's just return the matches and the CLI can handle the mapping
        // or we can implement a helper that queries the index.
    ) -> Result<Vec<(std::path::PathBuf, usize, String)>> {
        let re = RegexBuilder::new(pattern)
            .case_insensitive(true) // Default to smart case or case insensitive? Spec says "regex/substring".
            .build()?;

        Self::walk_and_match(root, re, None, None, None)
    }

    pub fn search_with_ignore(
        root: &Path,
        pattern: &str,
        config: &IndexConfig,
        honor_ignore: bool,
    ) -> Result<Vec<(std::path::PathBuf, usize, String)>> {
        let re = RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()?;

        let mut builder = WalkBuilder::new(root);
        if honor_ignore {
            builder.git_ignore(true).git_exclude(true).ignore(true);
        } else {
            builder.git_ignore(false).git_exclude(false).ignore(false);
        }

        if honor_ignore {
            let include_set = if config.include_paths.is_empty() {
                build_globset(vec!["**/*".to_string()])
            } else {
                build_globset(config.include_paths.clone())
            };
            let mut exclude_patterns: Vec<String> = config.exclude_paths.clone();
            exclude_patterns.extend(["node_modules/**", "dist/**", "build/**", "target/**", ".git/**"].iter().map(|s| s.to_string()));
            let exclude_set = build_globset(exclude_patterns);

            return Self::walk_and_match(root, re, include_set, exclude_set, None);
        }

        Self::walk_and_match(root, re, None, None, Some(builder))
    }

    fn walk_and_match(
        root: &Path,
        re: regex::Regex,
        include_set: Option<globset::GlobSet>,
        exclude_set: Option<globset::GlobSet>,
        builder_override: Option<WalkBuilder>,
    ) -> Result<Vec<(std::path::PathBuf, usize, String)>> {
        let mut matches = Vec::new();

        let builder = if let Some(b) = builder_override {
            b
        } else {
            let mut b = WalkBuilder::new(root);
            b.git_ignore(true).git_exclude(true).ignore(true);
            b
        };

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

                        if let Ok(file) = File::open(path) {
                            let reader = BufReader::new(file);
                            for (line_idx, line) in reader.lines().enumerate() {
                                if let Ok(line_content) = line {
                                    if re.is_match(&line_content) {
                                        matches.push((
                                            path.to_path_buf(),
                                            line_idx + 1,
                                            line_content,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        Ok(matches)
    }
}
