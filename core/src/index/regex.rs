use crate::config::IndexConfig;
use crate::models::Chunk;
use anyhow::Result;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

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
        let mut matches = Vec::new();
        
        let re = RegexBuilder::new(pattern)
            .case_insensitive(true) // Default to smart case or case insensitive? Spec says "regex/substring".
            .build()?;

        let builder = WalkBuilder::new(root);
        // Apply config ignores (same as scanner)
        // ...

        let walker = builder.build();

        for result in walker {
            match result {
                Ok(entry) => {
                    if entry.file_type().map_or(false, |ft| ft.is_file()) {
                        let path = entry.path();
                        // Check extension/language via config if needed
                        
                        if let Ok(file) = File::open(path) {
                            let reader = BufReader::new(file);
                            for (line_idx, line) in reader.lines().enumerate() {
                                if let Ok(line_content) = line {
                                    if re.is_match(&line_content) {
                                        matches.push((path.to_path_buf(), line_idx + 1, line_content));
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
