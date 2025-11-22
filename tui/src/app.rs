use coderet_core::retriever::{Retriever, SearchResult};
use coderet_core::config::Config;
use std::sync::Arc;
use std::process::Command;
use std::path::PathBuf;

pub enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    pub input: String,
    pub input_mode: InputMode,
    pub results: Vec<SearchResult>,
    pub selected_result_index: Option<usize>,
    pub retriever: Option<Retriever>,
    pub status_message: String,
}

impl App {
    pub fn new() -> App {
        // We should load config and retriever here or pass it in.
        // For simplicity, we'll try to load default config and initialize retriever if index exists.
        // But initializing retriever might be slow/async.
        // For now, we start with empty retriever and maybe load it on first search or init.
        
        let mut app = App {
            input: String::new(),
            input_mode: InputMode::Normal,
            results: Vec::new(),
            selected_result_index: None,
            retriever: None,
            status_message: "Press 'i' to index, 's' to search, 'q' to quit.".to_string(),
        };
        
        // Try to init retriever
        if let Ok(_config) = Config::load() { // Assuming Config::load exists or we use default
             // We need to know where the index is. Config doesn't store index path directly usually, 
             // but we can assume default `.codeindex`.
             let index_dir = std::path::Path::new(".codeindex");
             if index_dir.exists() {
                 // We need to initialize retriever.
                 // Retriever::new takes lexical and vector indices.
                 // This logic is duplicated from CLI. Ideally we should share it.
                 // For now, we'll skip auto-init or do it simply.
                 // Let's just set status.
                 app.status_message = "Index found. Press 's' to search.".to_string();
             } else {
                 app.status_message = "No index found. Press 'i' to index.".to_string();
             }
        }
        
        app
    }

    pub fn on_key(&mut self, c: char) {
        match self.input_mode {
            InputMode::Normal => {
                match c {
                    'i' => {
                        // Trigger indexing? 
                        // Indexing is a heavy operation. 
                        // We probably shouldn't do it in TUI main thread without async/loading state.
                        // For Phase 1, maybe just tell user to run CLI index?
                        self.status_message = "Please run 'code-retriever index' from CLI to index.".to_string();
                    }
                    's' => {
                        self.input_mode = InputMode::Editing;
                        self.status_message = "Enter query...".to_string();
                    }
                    'j' => self.select_next(),
                    'k' => self.select_prev(),
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
        self.status_message = "Normal mode.".to_string();
    }

    pub fn on_up(&mut self) {
        if let InputMode::Normal = self.input_mode {
            self.select_prev();
        }
    }

    pub fn on_down(&mut self) {
        if let InputMode::Normal = self.input_mode {
            self.select_next();
        }
    }

    fn select_next(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let idx = self.selected_result_index.unwrap_or(0);
        let next = (idx + 1).min(self.results.len().saturating_sub(1));
        self.selected_result_index = Some(next);
    }

    fn select_prev(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let idx = self.selected_result_index.unwrap_or(0);
        let prev = idx.saturating_sub(1);
        self.selected_result_index = Some(prev);
    }
    
    pub async fn on_enter(&mut self) {
        if let InputMode::Editing = self.input_mode {
            self.input_mode = InputMode::Normal;
            self.status_message = format!("Searching for: {}", self.input);
            // Perform search
            self.perform_search().await;
        }
    }
    
    async fn perform_search(&mut self) {
        // Initialize retriever if needed
        if self.retriever.is_none() {
             let branch = current_branch().unwrap_or_else(|| "default".to_string());
             let index_dir = PathBuf::from(".codeindex").join("branches").join(branch);
             if !index_dir.exists() {
                 self.status_message = "Index not found. Run 'code-retriever index' first.".to_string();
                 return;
             }
             
             // Load config
             let config = Config::load().unwrap_or_default();
             
             // Initialize components
             let lexical_index = match coderet_core::index::lexical::LexicalIndex::new(&index_dir.join("lexical")) {
                 Ok(idx) => Arc::new(idx),
                 Err(e) => {
                     self.status_message = format!("Failed to load lexical index: {}", e);
                     return;
                 }
             };
             
             let vector_index = match coderet_core::index::vector::VectorIndex::new(&index_dir.join("vector.lance")).await {
                 Ok(idx) => Arc::new(idx),
                 Err(e) => {
                     self.status_message = format!("Failed to load vector index: {}", e);
                     return;
                 }
             };
             
             let embedder: Arc<dyn coderet_core::embeddings::Embedder + Send + Sync> = match coderet_core::embeddings::external::ExternalEmbedder::new(None) {
                 Ok(e) => Arc::new(e),
                 Err(e) => {
                     // If external fails (no API key), we can't do semantic search.
                     // We should handle this gracefully.
                     // For now, we'll create a dummy embedder or just fail semantic.
                     // But Retriever needs an embedder.
                     // Let's try to proceed? No, Retriever needs it.
                     // We'll set status and return.
                     self.status_message = format!("Semantic search unavailable: {}", e);
                     // We can still do lexical search if we had a way to disable semantic in Retriever.
                     // But Retriever constructor takes embedder.
                     // We'll fail for now.
                     return;
                 }
             };
             
             let retriever = Retriever::new(lexical_index, vector_index, embedder, None, None, None, config);
             self.retriever = Some(retriever);
        }
        
        if let Some(retriever) = &self.retriever {
            // Perform search
            // We need to know the mode. Default to Hybrid.
            let config = Config::load().unwrap_or_default();
            let mode = config.search.default_mode;
            let top_k = config.search.default_top_k;
            
            // Map config mode to core mode
            let core_mode = match mode {
                coderet_core::config::SearchMode::Lexical => coderet_core::config::SearchMode::Lexical,
                coderet_core::config::SearchMode::Semantic => coderet_core::config::SearchMode::Semantic,
                coderet_core::config::SearchMode::Hybrid => coderet_core::config::SearchMode::Hybrid,
            };
            
            match retriever.search(&self.input, core_mode, top_k).await {
                Ok(results) => {
                    self.results = results;
                    if !self.results.is_empty() {
                        self.selected_result_index = Some(0);
                    }
                    self.status_message = format!("Found {} results.", self.results.len());
                }
                Err(e) => {
                    self.status_message = format!("Search failed: {}", e);
                }
            }
        }
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
                if let Ok(out2) = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output() {
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
