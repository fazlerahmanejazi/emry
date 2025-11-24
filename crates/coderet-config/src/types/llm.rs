//! LLM configuration

use serde::{Deserialize, Serialize};

/// LLM (Large Language Model) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Model name
    ///
    /// Examples: "gpt-4o-mini", "gpt-4o", "gpt-3.5-turbo"
    #[serde(default = "default_model")]
    pub model: String,

    /// Maximum tokens for LLM responses
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Optional API base URL override
    ///
    /// Use this to point to alternative OpenAI-compatible endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            max_tokens: default_max_tokens(),
            api_base: None,
        }
    }
}

impl crate::validation::Validate for LlmConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;

        if self.model.is_empty() {
            return Err(ConfigError::ValidationError {
                field: "llm.model".to_string(),
                message: "Model name cannot be empty".to_string(),
            });
        }

        if self.max_tokens == 0 {
            return Err(ConfigError::ValidationError {
                field: "llm.max_tokens".to_string(),
                message: "max_tokens must be > 0".to_string(),
            });
        }

        // Validate API base URL if provided
        if let Some(api_base) = &self.api_base {
            if api_base.is_empty() {
                return Err(ConfigError::ValidationError {
                    field: "llm.api_base".to_string(),
                    message: "API base URL cannot be empty string (use null to unset)".to_string(),
                });
            }

            // Basic URL validation
            if !api_base.starts_with("http://") && !api_base.starts_with("https://") {
                return Err(ConfigError::ValidationError {
                    field: "llm.api_base".to_string(),
                    message: format!(
                        "API base must start with http:// or https://, got: {}",
                        api_base
                    ),
                });
            }
        }

        Ok(())
    }
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_max_tokens() -> u32 {
    800
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = LlmConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_model_invalid() {
        let config = LlmConfig {
            model: String::new(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_max_tokens_invalid() {
        let config = LlmConfig {
            max_tokens: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_api_base() {
        let config = LlmConfig {
            api_base: Some("not-a-url".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_valid_api_base() {
        let config = LlmConfig {
            api_base: Some("https://api.openai.com/v1".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }
}
