use coderet_core::config::Config;
use coderet_core::agent::brain::{Agent, AgentOptions};
use coderet_core::agent::llm::LLMClient;
use coderet_core::agent::tools::{ToolRegistry, SearchTool, PathTool, ReadCodeTool, GrepTool, SymbolTool, ReferencesTool, ListDirTool, SummaryTool};
use coderet_core::index::lexical::LexicalIndex;
use coderet_core::index::vector::VectorIndex;
use coderet_core::structure::index::SymbolIndex;
use coderet_core::structure::graph::CodeGraph;
use coderet_core::retriever::Retriever;
use coderet_core::ranking::model::{Ranker, LinearRanker};
use coderet_core::summaries::index::SummaryIndex;
use coderet_core::summaries::vector::SummaryVectorIndex;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

pub enum InputMode {
    Normal,
    Editing,
}

#[derive(Clone)]
pub struct Message {
    pub role: String, // "user", "agent", "system"
    pub content: String,
}

pub struct App {
    pub input: String,
    pub input_mode: InputMode,
    pub messages: Vec<Message>,
    pub agent: Option<Agent>,
    pub status_message: String,
    pub scroll_offset: usize,
}

impl App {
    pub fn new() -> App {
        App {
            input: String::new(),
            input_mode: InputMode::Normal,
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: "Agent TUI - Press 'a' to ask a question, 'q' to quit.".to_string(),
                }
            ],
            agent: None,
            status_message: "Initializing...".to_string(),
            scroll_offset: 0,
        }
    }

    pub fn on_key(&mut self, c: char) {
        match self.input_mode {
            InputMode::Normal => {
                match c {
                    'a' => {
                        self.input_mode = InputMode::Editing;
                        self.status_message = "Enter your question...".to_string();
                    }
                    'j' => self.scroll_down(),
                    'k' => self.scroll_up(),
                    _ => {}
                }
            }
            InputMode::Editing => {
                // Handled in on_char
            }
        }
    }

    pub fn on_char(&mut self, c: char) {
        if let InputMode::Editing = self.input_mode {
            self.input.push(c);
        }
    }

    pub fn on_backspace(&mut self) {
        if let InputMode::Editing = self.input_mode {
            self.input.pop();
        }
    }

    pub fn on_esc(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input.clear();
        self.status_message = "Normal mode.".to_string();
    }

    pub fn on_up(&mut self) {
        if let InputMode::Normal = self.input_mode {
            self.scroll_up();
        }
    }

    pub fn on_down(&mut self) {
        if let InputMode::Normal = self.input_mode {
            self.scroll_down();
        }
    }

    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub async fn on_enter(&mut self) {
        if let InputMode::Editing = self.input_mode {
            self.input_mode = InputMode::Normal;
            let query = self.input.clone();
            self.input.clear();
            
            if query.trim().is_empty() {
                self.status_message = "Query cannot be empty".to_string();
                return;
            }

            // Add user message
            self.messages.push(Message {
                role: "user".to_string(),
                content: query.clone(),
            });

            self.status_message = "Agent is thinking...".to_string();
            
            // Initialize agent if needed
            if self.agent.is_none() {
                if let Err(e) = self.initialize_agent().await {
                    self.messages.push(Message {
                        role: "system".to_string(),
                        content: format!("Error initializing agent: {}", e),
                    });
                    self.status_message = "Error initializing agent".to_string();
                    return;
                }
            }

            // Ask agent
            if let Some(agent) = &self.agent {
                match agent.ask(&query, AgentOptions::default()).await {
                    Ok(answer) => {
                        self.messages.push(Message {
                            role: "agent".to_string(),
                            content: answer,
                        });
                        self.status_message = "Ready".to_string();
                    }
                    Err(e) => {
                        self.messages.push(Message {
                            role: "system".to_string(),
                            content: format!("Error: {}", e),
                        });
                        self.status_message = "Error".to_string();
                    }
                }
            }
        }
    }

    async fn initialize_agent(&mut self) -> anyhow::Result<()> {
        let branch = current_branch().unwrap_or_else(|| "default".to_string());
        let index_dir = PathBuf::from(".codeindex").join("branches").join(branch);
        
        if !index_dir.exists() {
            return Err(anyhow::anyhow!("Index not found. Run 'coderet index' first."));
        }

        let config = Config::load().unwrap_or_default();

        // Initialize components
        let lexical_index = Arc::new(LexicalIndex::new(&index_dir.join("lexical"))?);
        let vector_index = Arc::new(VectorIndex::new(&index_dir.join("vector.lance")).await?);
        
        let embedder = crate::embeddings_util::select_embedder(&config.embeddings)
            .ok_or_else(|| anyhow::anyhow!("No suitable embedder found."))?;

        let symbol_index = SymbolIndex::load(&index_dir.join("symbols.json")).ok().map(Arc::new);
        let graph = CodeGraph::load(&index_dir.join("graph.json")).ok().map(Arc::new);
        let summary_index = SummaryIndex::load(&index_dir.join("summaries.json")).ok().map(Arc::new);
        let summary_vector = SummaryVectorIndex::new(&index_dir.join("summary.lance")).await.ok();
        let ranker: Option<Box<dyn Ranker + Send + Sync>> = Some(Box::new(LinearRanker::default()));

        let retriever = Arc::new(Retriever::new(
            lexical_index,
            vector_index,
            embedder,
            symbol_index.clone(),
            summary_index.clone(),
            graph.clone(),
            ranker,
            summary_vector,
        ));

        // Setup Tools
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(SearchTool::new(retriever)));
        registry.register(Arc::new(ReadCodeTool::new()));
        registry.register(Arc::new(GrepTool::new()));
        registry.register(Arc::new(ReferencesTool::new()));
        registry.register(Arc::new(ListDirTool::new()));
        if let Some(sum) = summary_index {
            registry.register(Arc::new(SummaryTool::new(sum)));
        }
        
        if let Some(idx) = symbol_index {
            registry.register(Arc::new(SymbolTool::new(idx)));
        }
        
        if let Some(g) = graph {
            registry.register(Arc::new(PathTool::new(g)));
        }

        // Setup Agent
        let llm = LLMClient::new(None)?;
        let agent = Agent::new(llm, registry);
        
        self.agent = Some(agent);
        self.status_message = "Agent initialized".to_string();
        
        Ok(())
    }
}

fn current_branch() -> Option<String> {
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
    {
        if output.status.success() {
            let mut name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if name == "HEAD" {
                if let Ok(out2) = Command::new("git")
                    .args(["rev-parse", "--short", "HEAD"])
                    .output()
                {
                    if out2.status.success() {
                        name = format!("detached_{}", String::from_utf8_lossy(&out2.stdout).trim());
                    }
                }
            }
            return Some(name.replace('/', "__").replace(' ', "_"));
        }
    }
    None
}
