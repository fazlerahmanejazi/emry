use serde::{Deserialize, Serialize};
use crate::summaries::index::SummaryLevel;

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
    #[serde(default)]
    pub summaries: SummariesConfig,
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
    #[serde(default = "default_summary_boost_weight")]
    pub summary_boost_weight: f32,
    #[serde(default = "default_summary_similarity_threshold")]
    pub summary_similarity_threshold: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_mode: SearchMode::Hybrid,
            default_top_k: 10,
            summary_boost_weight: default_summary_boost_weight(),
            summary_similarity_threshold: default_summary_similarity_threshold(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummariesConfig {
    #[serde(default = "default_summaries_enabled")]
    pub enabled: bool,
    #[serde(default = "default_summary_levels")]
    pub levels: Vec<SummaryLevel>,
    #[serde(default = "default_summary_model")]
    pub model: String,
    #[serde(default = "default_summary_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_summary_prompt_version")]
    pub prompt_version: String,
    #[serde(default = "default_summary_retries")]
    pub retries: u8,
    #[serde(default = "default_auto_on_query")]
    pub auto_on_query: bool,
}

impl Default for SummariesConfig {
    fn default() -> Self {
        Self {
            enabled: default_summaries_enabled(),
            levels: default_summary_levels(),
            model: default_summary_model(),
            max_tokens: default_summary_max_tokens(),
            prompt_version: default_summary_prompt_version(),
            retries: default_summary_retries(),
            auto_on_query: default_auto_on_query(),
        }
    }
}

fn default_summaries_enabled() -> bool {
    true
}

fn default_summary_levels() -> Vec<SummaryLevel> {
    vec![
        SummaryLevel::Function,
        SummaryLevel::Class,
        SummaryLevel::File,
    ]
}

fn default_summary_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_summary_max_tokens() -> usize {
    160
}

fn default_summary_prompt_version() -> String {
    "v1".to_string()
}

fn default_summary_retries() -> u8 {
    2
}

fn default_summary_boost_weight() -> f32 {
    0.1
}

fn default_summary_similarity_threshold() -> f32 {
    0.25
}

fn default_auto_on_query() -> bool {
    true
}
