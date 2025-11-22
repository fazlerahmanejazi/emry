use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    pub max_tokens: usize,
    pub overlap_tokens: usize,
    pub split_strategy: SplitStrategy,
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
            max_tokens: 512,        // Safe for most embedding models
            overlap_tokens: 50,     // Context preservation
            split_strategy: SplitStrategy::Split,
        }
    }
}
