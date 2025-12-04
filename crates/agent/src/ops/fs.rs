use crate::project::context::RepoContext;
use anyhow::{anyhow, Result};
use emry_core::models::Language;
use emry_core::symbols;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use ignore::WalkBuilder;

use serde::Serialize;

#[derive(Clone)]
pub struct FsTool {
    ctx: Arc<RepoContext>,
}

#[derive(Debug, Clone, Serialize)]
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
        
        let resolved = if path.is_relative() {
            workspace_root.join(path)
        } else {
            path.to_path_buf()
        };
        
        let canonical = resolved.canonicalize().map_err(|e| {
            anyhow!(
                "Path does not exist or is inaccessible: {} ({})", 
                resolved.display(), 
                e
            )
        })?;
        
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
        let validated_path = self.validate_and_resolve_path(dir_path)?;
        
        let mut entries = Vec::new();
        let mut count = 0;

        let exclude_patterns = &self.ctx.config.core.exclude_paths;
        let exclude_set = emry_core::scanner::build_globset(exclude_patterns.clone());

        let walker = WalkBuilder::new(&validated_path)
            .max_depth(Some(depth))
            .git_ignore(true)
            .ignore(true)
            .hidden(false) 
            .build();

        for result in walker {
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    
                    if path == validated_path {
                        continue;
                    }

                    if let Some(set) = &exclude_set {
                        if let Ok(rel_path) = path.strip_prefix(&self.ctx.root) {
                            if set.is_match(rel_path) {
                                continue;
                            }
                        }
                    }

                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    
                    entries.push(DirEntry {
                        path: path.to_path_buf(),
                        is_dir,
                    });

                    count += 1;
                    if let Some(max) = limit {
                        if count >= max {
                            break;
                        }
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }
        
        Ok(entries)
    }

    /// Read a span from a file.
    pub fn read_file_span(&self, path: &Path, start: usize, end: usize) -> Result<String> {
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

    /// Read multiple files concurrently.
    pub async fn read_files_concurrent(&self, paths: Vec<PathBuf>) -> HashMap<PathBuf, String> {
        let this = self.clone();
        
        let stream = stream::iter(paths)
            .map(move |path| {
                let this = this.clone();
                async move {
                    match this.validate_and_resolve_path(&path) {
                        Ok(resolved) => {
                            match tokio::fs::read_to_string(&resolved).await {
                                Ok(content) => Some((path, content)),
                                Err(_) => None,
                            }
                        },
                        Err(_) => None,
                    }
                }
            })
            .buffer_unordered(50);
            
        stream.collect::<Vec<_>>().await.into_iter().flatten().collect()
    }

    pub fn outline(&self, path: &Path) -> Result<Vec<emry_core::models::Symbol>> {
        let validated_path = self.validate_and_resolve_path(path)?;
        
        let text = fs::read_to_string(&validated_path)
            .map_err(|e| anyhow!("unable to read file {}: {}", validated_path.display(), e))?;

        let language = validated_path
            .extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        if language == Language::Unknown {
            return Ok(Vec::new());
        }

        let symbols = symbols::extract_symbols(&text, &validated_path, &language)?;
        Ok(symbols)
    }

    pub fn generate_outline(&self, path: &Path) -> Result<String> {
        let validated_path = self.validate_and_resolve_path(path)?;
        let text = fs::read_to_string(&validated_path)
            .map_err(|e| anyhow!("unable to read file {}: {}", validated_path.display(), e))?;

        let language = validated_path
            .extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        if language == Language::Unknown {
            return Ok("Cannot generate outline for unknown language".to_string());
        }

        symbols::generate_outline(&text, &validated_path, &language)
    }

    pub fn extract_code_item(&self, path: &Path, node_path: &str) -> Result<Option<String>> {
        let validated_path = self.validate_and_resolve_path(path)?;
        let text = fs::read_to_string(&validated_path)
            .map_err(|e| anyhow!("unable to read file {}: {}", validated_path.display(), e))?;

        let language = validated_path
            .extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        if language == Language::Unknown {
            return Ok(None);
        }

        symbols::extract_code_item(&text, &validated_path, &language, node_path)
    }

    pub fn generate_codebase_map(&self, max_depth: usize) -> Result<String> {
        let workspace_root = &self.ctx.root;
        let exclude_paths = &self.ctx.config.core.exclude_paths;
        emry_core::map::generate_codebase_map(workspace_root, max_depth, exclude_paths)
    }

    pub async fn explore_module(&self, path: &str, depth: usize) -> Result<String> {
        let dir_path = self.validate_and_resolve_path(Path::new(path))?;
        
        let entries = self.list_files(&dir_path, depth, Some(100))?;
        
        let mut out = String::new();
        out.push_str(&format!("Exploration of '{}':\n\n", path));
        
        out.push_str("File Tree:\n");

        
        for entry in &entries {
            let relative_path = entry.path.strip_prefix(&dir_path).unwrap_or(&entry.path);
            let prefix = if entry.is_dir { "DIR " } else { "FILE" };
            if !entry.is_dir {
                out.push_str(&format!("[{}] {}\n", prefix, relative_path.display()));
            }
        }
        
        Ok(out)
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use emry_config::Config;

    #[test]
    fn test_list_files_respects_ignore() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path().canonicalize()?;

        std::fs::create_dir(root.join(".git"))?;

        let gitignore = root.join(".gitignore");
        File::create(&gitignore)?.write_all(b"ignored.txt")?;

        File::create(root.join("ignored.txt"))?.write_all(b"ignored")?;
        File::create(root.join("visible.txt"))?.write_all(b"visible")?;

        let config = Config::default();
        let ctx = Arc::new(RepoContext {
            root: root.clone(),
            branch: "main".to_string(),
            index_dir: root.join(".codeindex"),
            config,
            embedder: None,
            surreal_store: None,
        });

        let fs_tool = FsTool::new(ctx);
        let files = fs_tool.list_files(&root, 1, None)?;

        let file_names: Vec<String> = files.iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(file_names.contains(&"visible.txt".to_string()), "Should contain visible.txt");
        assert!(!file_names.contains(&"ignored.txt".to_string()), "Should NOT contain ignored.txt");

        Ok(())
    }
}
