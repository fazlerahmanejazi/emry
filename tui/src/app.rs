use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use std::time::Duration;

#[derive(Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Search,
    Agent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgentStatus {
    Idle,
    Planning,
    Executing,
    Synthesizing,
    Done,
    Error,
}

pub struct App {
    pub input: String,
    pub messages: Vec<Message>,
    pub status: String,
    pub mode: AppMode,
    // Agent-specific fields
    pub agent_plan: Option<String>,
    pub agent_observations: Vec<String>,
    pub agent_answer: Option<String>,
    pub agent_status: AgentStatus,
}

impl App {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            messages: vec![Message {
                role: "system".to_string(),
                content: "Type a query and press Enter. Press 'a' for agent mode, 's' for search mode. Press 'q' to quit.".to_string(),
            }],
            status: "Ready (Search Mode)".to_string(),
            mode: AppMode::Search,
            agent_plan: None,
            agent_observations: Vec::new(),
            agent_answer: None,
            agent_status: AgentStatus::Idle,
        }
    }

    pub fn toggle_to_agent(&mut self) {
        self.mode = AppMode::Agent;
        self.status = "Ready (Agent Mode)".to_string();
        self.messages.push(Message {
            role: "system".to_string(),
            content:
                "Switched to Agent mode. Agent will plan, execute tools, and synthesize answers."
                    .to_string(),
        });
    }

    pub fn toggle_to_search(&mut self) {
        self.mode = AppMode::Search;
        self.status = "Ready (Search Mode)".to_string();
        self.messages.push(Message {
            role: "system".to_string(),
            content: "Switched to Search mode. Direct search results will be displayed."
                .to_string(),
        });
    }

    pub fn on_key(&mut self, code: KeyCode) -> Option<String> {
        match code {
            KeyCode::Char('q') => {
                return Some("__quit__".to_string());
            }
            KeyCode::Char('a') => {
                self.toggle_to_agent();
                return None;
            }
            KeyCode::Char('s') => {
                self.toggle_to_search();
                return None;
            }
            KeyCode::Enter => {
                let q = self.input.trim().to_string();
                self.input.clear();
                if !q.is_empty() {
                    self.messages.push(Message {
                        role: "user".to_string(),
                        content: q.clone(),
                    });
                    match self.mode {
                        AppMode::Search => {
                            self.status = "Searching...".to_string();
                        }
                        AppMode::Agent => {
                            self.status = "Planning...".to_string();
                            self.agent_status = AgentStatus::Planning;
                            self.agent_plan = None;
                            self.agent_observations.clear();
                            self.agent_answer = None;
                        }
                    }
                    return Some(q);
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => {}
        }
        None
    }

    pub fn on_agent_plan(&mut self, plan: String) {
        self.agent_plan = Some(plan.clone());
        self.agent_status = AgentStatus::Executing;
        self.status = "Executing plan...".to_string();
    }

    pub fn on_agent_observation(&mut self, obs: String) {
        self.agent_observations.push(obs);
    }

    pub fn on_agent_synthesizing(&mut self) {
        self.agent_status = AgentStatus::Synthesizing;
        self.status = "Synthesizing answer...".to_string();
    }

    pub fn on_agent_result(&mut self, answer: String) {
        self.agent_answer = Some(answer.clone());
        self.agent_status = AgentStatus::Done;
        self.messages.push(Message {
            role: "agent".to_string(),
            content: answer,
        });
        self.status = "Done (Agent Mode)".to_string();
    }

    pub fn on_result(&mut self, content: String) {
        self.messages.push(Message {
            role: "agent".to_string(),
            content,
        });
        self.status = match self.mode {
            AppMode::Search => "Ready (Search Mode)".to_string(),
            AppMode::Agent => "Ready (Agent Mode)".to_string(),
        };
    }

    pub fn on_error(&mut self, err: String) {
        self.messages.push(Message {
            role: "system".to_string(),
            content: err,
        });
        self.agent_status = AgentStatus::Error;
        self.status = "Error".to_string();
    }

    pub fn poll_event(timeout: Duration) -> Result<Option<Event>> {
        if event::poll(timeout)? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }
}
