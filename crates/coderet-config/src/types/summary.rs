//! Summary generation configuration

use serde::{Deserialize, Serialize};

/// Summary generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryConfig {
    /// Enable summary generation
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Which levels to generate summaries for
    #[serde(default = "default_levels")]
    pub levels: Vec<SummaryLevel>,

    /// Use LLM for summary generation
    ///
    /// When false: extracts first lines/symbol lists (fast, free)
    /// When true: uses LLM for semantic summaries (slower, costs $)
    #[serde(default = "default_use_llm")]
    pub use_llm: bool,

    /// LLM model for summary generation
    ///
    /// Only used if use_llm is true.
    /// Examples: "gpt-4o-mini", "gpt-3.5-turbo"
    #[serde(default = "default_model")]
    pub model: String,

    /// Maximum tokens for LLM summaries
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Prompt version (for A/B testing)
    #[serde(default = "default_prompt_version")]
    pub prompt_version: String,

    /// Number of retries for failed LLM calls
    #[serde(default = "default_retries")]
    pub retries: u8,

    /// Concurrent LLM requests for summary generation
    ///
    /// Controls how many summary generation requests run in parallel.
    /// Higher values = faster but may hit rate limits.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

/// Summary hierarchy levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SummaryLevel {
    /// Per-function summaries
    Function,
    /// Per-class summaries
    Class,
    /// Per-file summaries
    File,
    /// Per-module (directory) summaries
    Module,
    /// Repository-level summary
    Repo,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            levels: default_levels(),
            use_llm: default_use_llm(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            prompt_version: default_prompt_version(),
            retries: default_retries(),
            concurrency: default_concurrency(),
        }
    }
}

impl crate::validation::Validate for SummaryConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;
        use crate::validation::validate_positive;

        if self.enabled && self.levels.is_empty() {
            return Err(ConfigError::ValidationError {
                field: "summary.levels".to_string(),
                message: "Must specify at least one summary level when enabled".to_string(),
            });
        }

        if self.use_llm {
            if self.model.is_empty() {
                return Err(ConfigError::ValidationError {
                    field: "summary.model".to_string(),
                    message: "Model name required when use_llm is true".to_string(),
                });
            }

            validate_positive("summary.max_tokens", self.max_tokens, 0)?;
        }

        Ok(())
    }
}

fn default_enabled() -> bool {
    true
}

fn default_levels() -> Vec<SummaryLevel> {
    // Default to file, module, repo only (skip function/class for performance)
    vec![SummaryLevel::File, SummaryLevel::Module, SummaryLevel::Repo]
}

fn default_use_llm() -> bool {
    false // Fast text extraction by default
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_max_tokens() -> usize {
    160
}

fn default_prompt_version() -> String {
    "v1".to_string()
}

fn default_concurrency() -> usize {
    10
}

fn default_retries() -> u8 {
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = SummaryConfig::default();
        assert!(config.validate().is_ok());
        // Default should skip function/class for performance
        assert!(!config.levels.contains(&SummaryLevel::Function));
        assert!(!config.levels.contains(&SummaryLevel::Class));
    }

    #[test]
    fn test_enabled_without_levels_invalid() {
        let config = SummaryConfig {
            enabled: true,
            levels: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_use_llm_without_model_invalid() {
        let config = SummaryConfig {
            use_llm: true,
            model: String::new(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
