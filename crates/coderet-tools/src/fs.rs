use coderet_context::RepoContext;
use anyhow::{anyhow, Result};
use coderet_core::models::Language; // Added import
use coderet_core::symbols; // Added import
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize; // Added import

pub struct FsTool {
    ctx: Arc<RepoContext>,
}

#[derive(Debug, Clone, Serialize)] // Added Serialize
pub struct DirEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl FsTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    /// List entries (files and directories) in a given path.
    pub fn list_files(&self, dir_path: &Path, limit: Option<usize>) -> Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        let mut count = 0;

        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

            entries.push(DirEntry {
                path: path.clone(),
                is_dir: file_type.is_dir(),
            });

            count += 1;
            if let Some(max) = limit {
                if count >= max {
                    break;
                }
            }
        }
        Ok(entries)
    }

    /// Read a span from a file using blob/content store fallback to filesystem.
    pub fn read_file_span(&self, path: &Path, start: usize, end: usize) -> Result<String> {
        let text = self
            .ctx
            .file_blob_store
            .get_for_path(path)
            .ok()
            .flatten()
            .or_else(|| {
                // content store keyed by hash is harder without metadata; use fs fallback.
                fs::read_to_string(path).ok()
            })
            .ok_or_else(|| anyhow!("unable to read file {}", path.display()))?;

        let lines: Vec<&str> = text.lines().collect();
        if start == 0 || end == 0 || start > lines.len() {
            return Ok(text);
        }
        let s = start.saturating_sub(1);
        let e = end.min(lines.len());
        Ok(lines[s..e].join("\n"))
    }

    // Outline method implementation
    pub fn outline(&self, path: &Path) -> Result<Vec<coderet_core::models::Symbol>> {
        let text = self
            .ctx
            .file_blob_store
            .get_for_path(path)
            .ok()
            .flatten()
            .or_else(|| {
                fs::read_to_string(path).ok()
            })
            .ok_or_else(|| anyhow!("unable to read file {}", path.display()))?;

        let language = path
            .extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        if language == Language::Unknown {
            return Ok(Vec::new()); // Cannot extract symbols for unknown language
        }

        let symbols = symbols::extract_symbols(&text, path, &language)?;
        Ok(symbols)
    }
}

use crate::FsToolTrait;

impl FsToolTrait for FsTool {
    fn list_files(&self, dir_path: &Path, limit: Option<usize>) -> Result<Vec<DirEntry>> {
        FsTool::list_files(self, dir_path, limit)
    }

    fn outline(&self, path: &Path) -> Result<Vec<coderet_core::models::Symbol>> {
        FsTool::outline(self, path)
    }

    fn read_file_span(&self, path: &Path, start: usize, end: usize) -> Result<String> {
        FsTool::read_file_span(self, path, start, end)
    }
}
