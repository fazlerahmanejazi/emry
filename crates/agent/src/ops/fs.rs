use crate::project::context::RepoContext;
use anyhow::{anyhow, Result};
use emry_core::models::Language; // Added import
use emry_core::symbols; // Added import
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

    /// Validates and resolves a path to ensure it's within the workspace.
    /// Handles both relative and absolute paths.
    fn validate_and_resolve_path(&self, path: &Path) -> Result<PathBuf> {
        let workspace_root = &self.ctx.root;
        
        // Resolve relative paths against workspace root
        let resolved = if path.is_relative() {
            workspace_root.join(path)
        } else {
            path.to_path_buf()
        };
        
        // Canonicalize to resolve symlinks and .. references
        let canonical = resolved.canonicalize().map_err(|e| {
            anyhow!(
                "Path does not exist or is inaccessible: {} ({})", 
                resolved.display(), 
                e
            )
        })?;
        
        // Validate the path is within workspace
        if !canonical.starts_with(workspace_root) {
            return Err(anyhow!(
                "Access denied: path '{}' is outside workspace '{}'",
                canonical.display(),
                workspace_root.display()
            ));
        }
        
        Ok(canonical)
    }

    /// List entries (files and directories) in a given path.
    /// 
    /// # Arguments
    /// * `dir_path` - The directory to list
    /// * `depth` - How deep to recurse (1 = immediate children only, 2 = children + grandchildren, etc.)
    /// * `limit` - Optional maximum number of entries to return
    pub fn list_files(&self, dir_path: &Path, depth: usize, limit: Option<usize>) -> Result<Vec<DirEntry>> {
        // Validate path is within workspace
        let validated_path = self.validate_and_resolve_path(dir_path)?;
        
        let mut entries = Vec::new();
        let mut count = 0;
        
        self.list_files_recursive(&validated_path, depth, 1, &mut entries, &mut count, limit)?;
        
        Ok(entries)
    }
    
    /// Helper function for recursive directory listing
    fn list_files_recursive(
        &self,
        dir_path: &Path,
        max_depth: usize,
        current_depth: usize,
        entries: &mut Vec<DirEntry>,
        count: &mut usize,
        limit: Option<usize>,
    ) -> Result<()> {
        // Check if we've hit the limit
        if let Some(max) = limit {
            if *count >= max {
                return Ok(());
            }
        }
        
        // Check if we've exceeded max depth
        if current_depth > max_depth {
            return Ok(());
        }
        
        // Read directory entries
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            let is_dir = file_type.is_dir();
            
            // Add this entry
            entries.push(DirEntry {
                path: path.clone(),
                is_dir,
            });
            
            *count += 1;
            
            // Check limit again after adding
            if let Some(max) = limit {
                if *count >= max {
                    return Ok(());
                }
            }
            
            // Recurse into subdirectories if we haven't reached max depth
            if is_dir && current_depth < max_depth {
                self.list_files_recursive(
                    &path,
                    max_depth,
                    current_depth + 1,
                    entries,
                    count,
                    limit,
                )?;
            }
        }
        
        Ok(())
    }

    /// Read a span from a file.
    pub fn read_file_span(&self, path: &Path, start: usize, end: usize) -> Result<String> {
        // Validate path is within workspace
        let validated_path = self.validate_and_resolve_path(path)?;
        
        let text = fs::read_to_string(&validated_path)
            .map_err(|e| anyhow!("unable to read file {}: {}", validated_path.display(), e))?;

        let lines: Vec<&str> = text.lines().collect();
        if start == 0 || end == 0 || start > lines.len() {
            return Ok(text);
        }
        let s = start.saturating_sub(1);
        let e = end.min(lines.len());
        Ok(lines[s..e].join("\n"))
    }

    // Outline method implementation
    pub fn outline(&self, path: &Path) -> Result<Vec<emry_core::models::Symbol>> {
        // Validate path is within workspace
        let validated_path = self.validate_and_resolve_path(path)?;
        
        let text = fs::read_to_string(&validated_path)
            .map_err(|e| anyhow!("unable to read file {}: {}", validated_path.display(), e))?;

        let language = validated_path
            .extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        if language == Language::Unknown {
            return Ok(Vec::new()); // Cannot extract symbols for unknown language
        }

        let symbols = symbols::extract_symbols(&text, &validated_path, &language)?;
        Ok(symbols)
    }
}

pub trait FsToolTrait: Send + Sync {
    fn list_files(&self, dir_path: &Path, depth: usize, limit: Option<usize>) -> Result<Vec<DirEntry>>;
    fn outline(&self, path: &Path) -> Result<Vec<emry_core::models::Symbol>>;
    fn read_file_span(
        &self,
        path: &std::path::Path,
        start: usize,
        end: usize,
    ) -> Result<String>;
}

impl FsToolTrait for FsTool {
    fn list_files(&self, dir_path: &Path, depth: usize, limit: Option<usize>) -> Result<Vec<DirEntry>> {
        FsTool::list_files(self, dir_path, depth, limit)
    }

    fn outline(&self, path: &Path) -> Result<Vec<emry_core::models::Symbol>> {
        FsTool::outline(self, path)
    }

    fn read_file_span(&self, path: &Path, start: usize, end: usize) -> Result<String> {
        FsTool::read_file_span(self, path, start, end)
    }
}
