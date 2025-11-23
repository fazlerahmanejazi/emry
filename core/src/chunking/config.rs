use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    pub max_tokens: usize,
    pub overlap_tokens: usize,
    pub split_strategy: SplitStrategy,
    #[serde(default)]
    pub use_cast_chunking: bool,
    /// Character budget for CAST-style chunking (non-whitespace chars)
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            max_tokens: 512,    // Safe for most embedding models
            overlap_tokens: 50, // Context preservation
            split_strategy: SplitStrategy::Split,
            use_cast_chunking: true,
            max_chars: default_max_chars(),
        }
    }
}

fn default_max_chars() -> usize {
    2000
}
