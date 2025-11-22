use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
    #[serde(default = "default_auto_index_on_search")]
    pub auto_index_on_search: bool,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(None)
    }

    pub fn load_from(path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let config_path = path.unwrap_or_else(|| std::path::Path::new(".code-retriever.yml"));
        if config_path.exists() {
            let content = std::fs::read_to_string(config_path)?;
            let config: Config = serde_yaml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default)]
    pub include_paths: Vec<String>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    // Add more as needed
    #[serde(default)]
    pub chunking: crate::chunking::ChunkingConfig,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            include_paths: vec!["**/*".to_string()],
            exclude_paths: vec![],
            chunking: crate::chunking::ChunkingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_mode: SearchMode,
    pub default_top_k: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_mode: SearchMode::Hybrid,
            default_top_k: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Lexical,
    Semantic,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    pub backend: EmbeddingBackend,
    pub model_name: String,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            // Runtime chooses external when OPENAI_API_KEY is set, otherwise Ollama.
            backend: EmbeddingBackend::Ollama,
            model_name: "nomic-embed-text".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingBackend {
    External,
    Local,
    Ollama,
}

fn default_auto_index_on_search() -> bool {
    true
}
