//! Configuration type definitions
//!
//! This module contains all configuration structures organized by concern.
//! Each type is self-contained with validation and sensible defaults.

pub mod agent;
pub mod bm25;
pub mod chunking;
pub mod core;
pub mod embedding;
pub mod graph;
pub mod llm;
pub mod ranking;
pub mod search;

// Re-export all types for convenience
pub use agent::AgentConfig;
pub use bm25::Bm25Config;
pub use chunking::{ChunkingConfig, SplitStrategy};
pub use core::CoreConfig;
pub use embedding::{EmbeddingBackend, EmbeddingConfig};
pub use graph::GraphConfig;
pub use llm::LlmConfig;
pub use ranking::RankingConfig;
pub use search::{SearchConfig, SearchMode};


use serde::{Deserialize, Serialize};

/// Main configuration struct aggregating all settings
///
/// This is the top-level configuration that users interact with.
/// It's organized by functional area for clarity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Core settings (paths, storage)
    #[serde(default)]
    pub core: CoreConfig,

    /// Search behavior
    #[serde(default)]
    pub search: SearchConfig,

    /// Ranking weights for hybrid search
    #[serde(default)]
    pub ranking: RankingConfig,

    /// BM25 algorithm parameters
    #[serde(default)]
    pub bm25: Bm25Config,

    /// Graph traversal settings
    #[serde(default)]
    pub graph: GraphConfig,

    /// Code chunking configuration
    #[serde(default)]
    pub chunking: ChunkingConfig,

    /// Embedding provider settings
    #[serde(default)]
    pub embedding: EmbeddingConfig,

    /// Agent behavior limits
    #[serde(default)]
    pub agent: AgentConfig,

    /// LLM settings
    #[serde(default)]
    pub llm: LlmConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            core: CoreConfig::default(),
            search: SearchConfig::default(),
            ranking: RankingConfig::default(),
            bm25: Bm25Config::default(),
            graph: GraphConfig::default(),
            chunking: ChunkingConfig::default(),
            embedding: EmbeddingConfig::default(),
            agent: AgentConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

impl crate::validation::Validate for Config {
    fn validate(&self) -> crate::error::Result<()> {
        // Validate each sub-config
        self.core.validate()?;
        self.search.validate()?;
        self.ranking.validate()?;
        self.bm25.validate()?;
        self.graph.validate()?;
        self.chunking.validate()?;
        self.embedding.validate()?;
        self.agent.validate()?;
        self.llm.validate()?;

        Ok(())
    }
}
