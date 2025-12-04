use crate::cortex::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::project::context::RepoContext;
use crate::ops::context::SmartContext;

pub struct FocusTool {
    context_ops: SmartContext,
}

impl FocusTool {
    pub fn new(ctx: Arc<RepoContext>) -> Result<Self> {
        Ok(Self { 
            context_ops: SmartContext::new(ctx)?,
        })
    }
}

#[async_trait]
impl Tool for FocusTool {
    fn name(&self) -> &str {
        "focus_on"
    }

    fn description(&self) -> &str {
        "Automatically gather relevant context for a given topic or task. It searches the codebase, expands the graph to find related files, and returns outlines of the most relevant files."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "The topic or task to focus on (e.g., 'authentication', 'fix login bug')."
                }
            },
            "required": ["topic"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let topic = args["topic"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'topic' argument"))?;
            
        self.context_ops.focus(topic, |_| {}).await
    }
}
