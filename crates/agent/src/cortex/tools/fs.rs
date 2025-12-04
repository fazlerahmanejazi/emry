use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use crate::ops::fs::FsTool as InnerFsTool;

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
        
        let depth = args["depth"].as_u64().unwrap_or(1) as usize;
        
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

pub struct ViewFileOutlineTool {
    inner: Arc<InnerFsTool>,
}

impl ViewFileOutlineTool {
    pub fn new(inner: Arc<InnerFsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ViewFileOutlineTool {
    fn name(&self) -> &str {
        "view_file_outline"
    }

    fn description(&self) -> &str {
        "View the skeletal outline of a file (imports, classes, functions) without implementation details. Use this to get a high-level overview before reading specific code items."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path WITHIN the workspace."
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
        
        (*self.inner).generate_outline(path)
    }
}

pub struct ViewCodeItemTool {
    inner: Arc<InnerFsTool>,
}

impl ViewCodeItemTool {
    pub fn new(inner: Arc<InnerFsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ViewCodeItemTool {
    fn name(&self) -> &str {
        "view_code_item"
    }

    fn description(&self) -> &str {
        "View the implementation of a specific code item (function, class, method) by its name or path (e.g., 'MyClass.myMethod')."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path containing the item."
                },
                "node_path": {
                    "type": "string",
                    "description": "The name or path of the item (e.g., 'MyClass', 'my_function', 'MyClass.method')."
                }
            },
            "required": ["path", "node_path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let node_path = args["node_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'node_path' argument"))?;
            
        let path = std::path::Path::new(path_str);
        
        match (*self.inner).extract_code_item(path, node_path)? {
            Some(content) => Ok(content),
            None => Ok(format!("Item '{}' not found in file '{}'. Try checking the outline first.", node_path, path_str)),
        }
    }
}

pub struct ViewCodebaseMapTool {
    inner: Arc<InnerFsTool>,
}

impl ViewCodebaseMapTool {
    pub fn new(inner: Arc<InnerFsTool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ViewCodebaseMapTool {
    fn name(&self) -> &str {
        "view_codebase_map"
    }

    fn description(&self) -> &str {
        "View a high-level map of the codebase structure, including top-level symbols for each file. Useful for orientation."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum depth to traverse. Default is 5.",
                    "default": 5
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let max_depth = args["max_depth"].as_u64().unwrap_or(5) as usize;
        (*self.inner).generate_codebase_map(max_depth)
    }
}
