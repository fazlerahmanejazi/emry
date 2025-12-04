use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use crate::ops::fs::FsTool;
use crate::ops::graph::GraphTool;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ReadFilesTool {
    inner: Arc<FsTool>,
}

impl ReadFilesTool {
    pub fn new(inner: Arc<FsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ReadFilesTool {
    fn name(&self) -> &str {
        "read_files"
    }

    fn description(&self) -> &str {
        "Read the content of one or more files from the filesystem. You must provide a list of absolute paths."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of absolute file paths to read."
                }
            },
            "required": ["paths"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let paths_val = args["paths"].as_array().ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;
        let paths: Vec<std::path::PathBuf> = paths_val.iter().filter_map(|v| v.as_str().map(std::path::PathBuf::from)).collect();
        
        let results = self.inner.read_files_concurrent(paths).await;
        
        let mut out = String::new();
        for (path, content) in results {
            out.push_str(&format!("--- {} ---\n{}\n\n", path.display(), content));
        }
        if out.is_empty() {
            out.push_str("No files read (paths might be invalid or empty).");
        }
        Ok(out)
    }
}

pub struct ExploreModuleTool {
    inner: Arc<FsTool>,
}

impl ExploreModuleTool {
    pub fn new(inner: Arc<FsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ExploreModuleTool {
    fn name(&self) -> &str {
        "explore_module"
    }

    fn description(&self) -> &str {
        "Explore a directory/module. Lists files and automatically reads key files (README, mod.rs, etc.) to give you context."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to explore."
                },
                "depth": {
                    "type": "integer",
                    "default": 1,
                    "description": "Depth of directory listing."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let depth = args["depth"].as_u64().unwrap_or(1) as usize;
        
        self.inner.explore_module(path, depth).await
    }
}

pub struct FindUsagesTool {
    inner: Arc<GraphTool>,
}

impl FindUsagesTool {
    pub fn new(inner: Arc<GraphTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for FindUsagesTool {
    fn name(&self) -> &str {
        "find_usages"
    }

    fn description(&self) -> &str {
        "Find all usages (callers/references) of a symbol and show the code snippets. Useful for 'Where is X used?'."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbol": {
                    "type": "string",
                    "description": "The symbol name to find usages for."
                }
            },
            "required": ["symbol"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let symbol = args["symbol"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'symbol' argument"))?;
        
        let snippets = self.inner.find_usages(symbol).await?;
        
        if snippets.is_empty() {
            return Ok(format!("No usages found for '{}'.", symbol));
        }

        let mut out = String::new();
        out.push_str(&format!("Usages of '{}':\n\n", symbol));
        
        for snippet in snippets {
            out.push_str(&format!(
                "File: {}:{}\n```\n{}\n```\n\n",
                snippet.file_path, snippet.line_number, snippet.code
            ));
        }
        
        Ok(out)
    }
}
