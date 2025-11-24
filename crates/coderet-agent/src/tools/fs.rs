use crate::context::RepoContext;
use anyhow::{anyhow, Result};
use globset::Glob;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::FsToolTrait;

pub struct FsTool {
    ctx: Arc<RepoContext>,
}

impl FsTool {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self { ctx }
    }

    /// List tracked files from the index, optionally capped.
    pub fn list_files(&self, limit: Option<usize>) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for meta in self.ctx.file_store.list_metadata()? {
            files.push(meta.path.clone());
            if let Some(max) = limit {
                if files.len() >= max {
                    break;
                }
            }
        }
        Ok(files)
    }

    /// List tracked files matching a glob pattern.
    pub fn list_files_matching(&self, pattern: &str, limit: Option<usize>) -> Result<Vec<PathBuf>> {
        let glob = Glob::new(pattern)
            .map_err(|e| anyhow!("invalid pattern {}: {}", pattern, e))?
            .compile_matcher();
        let mut files = Vec::new();
        for meta in self.ctx.file_store.list_metadata()? {
            if glob.is_match(&meta.path) {
                files.push(meta.path.clone());
                if let Some(max) = limit {
                    if files.len() >= max {
                        break;
                    }
                }
            }
        }
        Ok(files)
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
}

impl FsToolTrait for FsTool {
    fn list_files(&self, limit: Option<usize>) -> Result<Vec<PathBuf>> {
        FsTool::list_files(self, limit)
    }

    fn list_files_matching(&self, pattern: &str, limit: Option<usize>) -> Result<Vec<PathBuf>> {
        FsTool::list_files_matching(self, pattern, limit)
    }

    fn read_file_span(&self, path: &Path, start: usize, end: usize) -> Result<String> {
        FsTool::read_file_span(self, path, start, end)
    }
}
