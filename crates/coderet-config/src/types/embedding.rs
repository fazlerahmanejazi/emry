//! Embedding provider configuration

use serde::{Deserialize, Serialize};

/// Embedding provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Embedding backend to use
    #[serde(default)]
    pub backend: EmbeddingBackend,

    /// Model name for the selected backend
    ///
    /// Examples:
    /// - OpenAI: "text-embedding-3-small", "text-embedding-3-large"
    /// - Ollama: "nomic-embed-text", "mxbai-embed-large"
    #[serde(default = "default_model_name")]
    pub model_name: String,
}

/// Embedding backend options
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingBackend {
    /// OpenAI API (requires OPENAI_API_KEY)
    #[serde(rename = "openai")]
    External,

    /// Local fastembed (CPU-based, no API needed)
    Local,

    /// Local Ollama server
    Ollama,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            backend: EmbeddingBackend::Ollama,
            model_name: default_model_name(),
        }
    }
}

impl Default for EmbeddingBackend {
    fn default() -> Self {
        EmbeddingBackend::Ollama
    }
}

impl crate::validation::Validate for EmbeddingConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;

        if self.model_name.is_empty() {
            return Err(ConfigError::ValidationError {
                field: "embedding.model_name".to_string(),
                message: "Model name cannot be empty".to_string(),
            });
        }

        // Backend-specific validation
        match self.backend {
            EmbeddingBackend::External => {
                // Check if API key is set when using OpenAI
                if std::env::var("OPENAI_API_KEY").is_err() {
                    eprintln!(
                        "Warning: embedding.backend is 'openai' but OPENAI_API_KEY is not set"
                    );
                }
            }
            EmbeddingBackend::Ollama => {
                // No additional validation needed
            }
            EmbeddingBackend::Local => {
                // No additional validation needed
            }
        }

        Ok(())
    }
}

fn default_model_name() -> String {
    "nomic-embed-text".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = EmbeddingConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_model_name_invalid() {
        let config = EmbeddingConfig {
            model_name: String::new(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_backend_serialization() {
        assert_eq!(
            serde_json::to_string(&EmbeddingBackend::External).unwrap(),
            "\"openai\""
        );
        assert_eq!(
            serde_json::to_string(&EmbeddingBackend::Ollama).unwrap(),
            "\"ollama\""
        );
    }
}
