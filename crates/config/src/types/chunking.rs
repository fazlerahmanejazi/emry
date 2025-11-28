//! Code chunking configuration

use serde::{Deserialize, Serialize};

/// Configuration for code chunking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    /// Maximum tokens per chunk
    ///
    /// Should match embedding model's token limit.
    /// Common values:
    /// - 512: Safe for most models
    /// - 1024: For larger context models
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Overlap tokens between chunks
    ///
    /// Provides context continuity across chunks.
    /// Recommended: 10-20% of max_tokens
    #[serde(default = "default_overlap")]
    pub overlap_tokens: usize,

    /// Chunking strategy
    #[serde(default)]
    pub strategy: SplitStrategy,

    /// Use CAST (Context-Aware Syntax Tree) chunking
    ///
    /// When true, uses AST-based smart chunking.
    /// When false, falls back to token-based chunking.
    #[serde(default = "default_use_cast")]
    pub use_cast: bool,

    /// Maximum characters for CAST chunking
    ///
    /// Character budget for AST-based chunks (non-whitespace)
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

/// Chunking strategy when token limit is exceeded
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SplitStrategy {
    /// Drop overflow tokens
    Truncate,
    /// Split into multiple chunks with overlap
    Split,
    /// Try to split at semantic boundaries (statements, expressions)
    Hierarchical,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_max_tokens(),
            overlap_tokens: default_overlap(),
            strategy: SplitStrategy::Split,
            use_cast: default_use_cast(),
            max_chars: default_max_chars(),
        }
    }
}

impl Default for SplitStrategy {
    fn default() -> Self {
        SplitStrategy::Split
    }
}

impl crate::validation::Validate for ChunkingConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;
        use crate::validation::validate_positive;

        // max_tokens must be positive
        validate_positive("chunking.max_tokens", self.max_tokens, 0)?;

        // overlap must be less than max_tokens
        if self.overlap_tokens >= self.max_tokens {
            return Err(ConfigError::ValidationError {
                field: "chunking.overlap_tokens".to_string(),
                message: format!(
                    "overlap_tokens ({}) must be < max_tokens ({})",
                    self.overlap_tokens, self.max_tokens
                ),
            });
        }

        // max_chars must be positive if CAST is enabled
        if self.use_cast {
            validate_positive("chunking.max_chars", self.max_chars, 0)?;
        }

        Ok(())
    }
}

fn default_max_tokens() -> usize {
    512 // Safe for most embedding models
}

fn default_overlap() -> usize {
    50 // ~10% overlap for context
}

fn default_use_cast() -> bool {
    true // Use smart chunking by default
}

fn default_max_chars() -> usize {
    2000 // Character budget for CAST
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = ChunkingConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_overlap_too_large() {
        let config = ChunkingConfig {
            max_tokens: 100,
            overlap_tokens: 100, // Equal to max
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_strategy_serialization() {
        assert_eq!(
            serde_json::to_string(&SplitStrategy::Hierarchical).unwrap(),
            "\"hierarchical\""
        );
    }
}
