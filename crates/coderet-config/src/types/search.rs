//! Search configuration

use serde::{Deserialize, Serialize};

/// Search behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Search mode to use
    #[serde(default)]
    pub mode: SearchMode,

    /// Number of top results to return
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

/// Search mode enum
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Lexical (BM25) search only
    Lexical,
    /// Semantic (vector) search only
    Semantic,
    /// Hybrid (combine lexical + semantic)
    Hybrid,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            mode: SearchMode::Hybrid,
            top_k: default_top_k(),
        }
    }
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid
    }
}

impl crate::validation::Validate for SearchConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;

        if self.top_k == 0 {
            return Err(ConfigError::InvalidInteger {
                field: "search.top_k".to_string(),
                value: self.top_k,
                min: 0,
            });
        }

        if self.top_k > 1000 {
            return Err(ConfigError::ValidationError {
                field: "search.top_k".to_string(),
                message: format!("top_k too large ({}), consider using <= 1000", self.top_k),
            });
        }

        Ok(())
    }
}

fn default_top_k() -> usize {
    10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = SearchConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.mode, SearchMode::Hybrid);
        assert_eq!(config.top_k, 10);
    }

    #[test]
    fn test_zero_top_k_invalid() {
        let config = SearchConfig {
            top_k: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_large_top_k_warning() {
        let config = SearchConfig {
            top_k: 2000,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_search_mode_serialization() {
        assert_eq!(
            serde_json::to_string(&SearchMode::Hybrid).unwrap(),
            "\"hybrid\""
        );
        assert_eq!(
            serde_json::to_string(&SearchMode::Lexical).unwrap(),
            "\"lexical\""
        );
    }
}
