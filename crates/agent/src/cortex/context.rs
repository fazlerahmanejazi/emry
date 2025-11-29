use crate::cortex::tool::Tool;
use crate::project::context::RepoContext;
use emry_engine::search::service::SearchService;
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
    pub search_service: Arc<SearchService>,
    pub tools: HashMap<String, Arc<dyn Tool>>,
    pub history: Vec<Step>,
    pub memory: Vec<String>, // "Facts" derived from observations
    pub config: emry_config::AgentConfig,
}

impl AgentContext {
    pub fn new(
        repo_context: Arc<RepoContext>,
        search_service: Arc<SearchService>,
        config: emry_config::AgentConfig,
    ) -> Self {
        Self {
            repo_context,
            search_service,
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
