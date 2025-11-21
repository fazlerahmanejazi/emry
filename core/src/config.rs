use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = std::path::Path::new(".code-retriever.yml");
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
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            include_paths: vec!["**/*".to_string()],
            exclude_paths: vec![],
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
            backend: EmbeddingBackend::Local,
            model_name: "BGESmallENV15".to_string(), // Example default
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingBackend {
    External,
    Local,
}
