use anyhow::Result;
use std::path::{Path, PathBuf};
use crate::tags_extractor::TagsExtractor;
use crate::models::Language;

pub struct DiffAnalyzer {
    extractor: TagsExtractor,
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: PathBuf,
    pub changed_ranges: Vec<(usize, usize)>, // start_line, end_line
}

#[derive(Debug, Clone)]
pub struct AffectedSymbol {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
}

impl DiffAnalyzer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            extractor: TagsExtractor::new()?,
        })
    }

    /// Maps a list of file diffs to the symbols that were modified.
    pub fn find_affected_symbols(&mut self, diffs: &[FileDiff], root_path: &Path) -> Result<Vec<AffectedSymbol>> {
        let mut affected = Vec::new();

        for diff in diffs {
            let full_path = root_path.join(&diff.path);
            if !full_path.exists() { continue; }

            let content = std::fs::read_to_string(&full_path)?;
            let language = Language::from_path(&full_path);
            
            if language == Language::Unknown { continue; }

            let symbols = self.extractor.extract_symbols(&content, &full_path, &language)?;

            for symbol in symbols {
                for (start, end) in &diff.changed_ranges {
                    if symbol.start_line <= *end && symbol.end_line >= *start {
                        affected.push(AffectedSymbol {
                            name: symbol.name.clone(),
                            kind: symbol.kind.clone(),
                            file_path: diff.path.to_string_lossy().to_string(),
                            start_line: symbol.start_line,
                            end_line: symbol.end_line,
                        });
                        break;
                    }
                }
            }
        }

        Ok(affected)
    }
}
