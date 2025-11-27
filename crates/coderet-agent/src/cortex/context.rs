use crate::cortex::tool::Tool;
use coderet_context::RepoContext;
use coderet_pipeline::manager::IndexManager;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Step {
    pub step_id: usize,
    pub thought: String,
    pub action: String,
    pub args: serde_json::Value,
    pub observation: String,
    pub error: Option<String>,
}

pub struct AgentContext {
    pub repo_context: Arc<RepoContext>,
    pub index_manager: Arc<IndexManager>,
    pub tools: HashMap<String, Arc<dyn Tool>>,
    pub history: Vec<Step>,
    pub memory: Vec<String>, // "Facts" derived from observations
    pub config: coderet_config::AgentConfig,
}

impl AgentContext {
    pub fn new(
        repo_context: Arc<RepoContext>,
        index_manager: Arc<IndexManager>,
        config: coderet_config::AgentConfig,
    ) -> Self {
        Self {
            repo_context,
            index_manager,
            tools: HashMap::new(),
            history: Vec::new(),
            memory: Vec::new(),
            config,
        }
    }

    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn add_step(&mut self, step: Step) {
        self.history.push(step);
    }

    pub fn add_memory(&mut self, fact: String) {
        self.memory.push(fact);
    }
    
    pub fn get_tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }
}
