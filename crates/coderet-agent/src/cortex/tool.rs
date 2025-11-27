use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait Tool: Send + Sync {
    /// The name of the tool (e.g., "search_code").
    fn name(&self) -> &str;

    /// A description of what the tool does and how to use it.
    fn description(&self) -> &str;

    /// The JSON schema for the tool's arguments.
    fn schema(&self) -> Value;

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: Value) -> Result<String>;
}
