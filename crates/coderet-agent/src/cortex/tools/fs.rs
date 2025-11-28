use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use coderet_tools::fs::FsTool as InnerFsTool;

use serde_json::{json, Value};
use std::sync::Arc;

pub struct ReadFileTool {
    inner: Arc<InnerFsTool>,
}

impl ReadFileTool {
    pub fn new(inner: Arc<InnerFsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. You must provide the full absolute path."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path WITHIN the workspace. Use relative paths from workspace root or absolute paths within the workspace."
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional start line (1-indexed)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional end line (1-indexed)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let path = std::path::Path::new(path_str);
        let start = args["start_line"].as_u64().unwrap_or(0) as usize;
        let end = args["end_line"].as_u64().unwrap_or(0) as usize;

        (*self.inner).read_file_span(path, start, end)
    }
}

pub struct ListFilesTool {
    inner: Arc<InnerFsTool>,
}

impl ListFilesTool {
    pub fn new(inner: Arc<InnerFsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files in a directory."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path WITHIN the workspace. Use '.' for workspace root, or relative paths (e.g., 'src', 'lib', 'packages/auth', 'app/models')."
                },
                "depth": {
                    "type": "integer",
                    "default": 1
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let path = std::path::Path::new(path_str);
        
        // Get depth from args, default to 1 if not provided
        let depth = args["depth"].as_u64().unwrap_or(1) as usize;
        
        // Use a reasonable limit to prevent overwhelming results
        let limit = Some(100); 

        let entries = (*self.inner).list_files(path, depth, limit)?;
        
        let mut out = String::new();
        for entry in entries {
            let kind = if entry.is_dir { "DIR" } else { "FILE" };
            out.push_str(&format!("[{}] {}\n", kind, entry.path.display()));
        }
        Ok(out)
    }
}
