//! Configuration merging logic
//!
//! Merges configurations from multiple sources with proper precedence.

use crate::{types::*, Config};

/// Merge two configurations, with `overlay` taking precedence
///
/// This performs a deep merge where non-default values from `overlay`
/// override values in `base`.
pub fn merge(mut base: Config, overlay: Config) -> Config {
    // For each section, merge fields if they differ from defaults
    base.search = merge_search(base.search, overlay.search);
    base.ranking = merge_ranking(base.ranking, overlay.ranking);
    base.bm25 = merge_bm25(base.bm25, overlay.bm25);
    base.graph = merge_graph(base.graph, overlay.graph);
    base.chunking = merge_chunking(base.chunking, overlay.chunking);
    base.embedding = merge_embedding(base.embedding, overlay.embedding);
    base.agent = merge_agent(base.agent, overlay.agent);
    base.llm = merge_llm(base.llm, overlay.llm);
    base.core = merge_core(base.core, overlay.core);

    base
}

fn merge_search(base: SearchConfig, overlay: SearchConfig) -> SearchConfig {
    let default = SearchConfig::default();
    SearchConfig {
        mode: if overlay.mode != default.mode {
            overlay.mode
        } else {
            base.mode
        },
        top_k: if overlay.top_k != default.top_k {
            overlay.top_k
        } else {
            base.top_k
        },
    }
}

fn merge_ranking(base: RankingConfig, overlay: RankingConfig) -> RankingConfig {
    let default = RankingConfig::default();
    RankingConfig {
        lexical: if (overlay.lexical - default.lexical).abs() > 0.001 {
            overlay.lexical
        } else {
            base.lexical
        },
        vector: if (overlay.vector - default.vector).abs() > 0.001 {
            overlay.vector
        } else {
            base.vector
        },
        graph: if (overlay.graph - default.graph).abs() > 0.001 {
            overlay.graph
        } else {
            base.graph
        },
        symbol: if (overlay.symbol - default.symbol).abs() > 0.001 {
            overlay.symbol
        } else {
            base.symbol
        },
    }
}

fn merge_bm25(base: Bm25Config, overlay: Bm25Config) -> Bm25Config {
    let default = Bm25Config::default();
    Bm25Config {
        k1: if (overlay.k1 - default.k1).abs() > 0.001 {
            overlay.k1
        } else {
            base.k1
        },
        b: if (overlay.b - default.b).abs() > 0.001 {
            overlay.b
        } else {
            base.b
        },
        avg_len: if overlay.avg_len != default.avg_len {
            overlay.avg_len
        } else {
            base.avg_len
        },
    }
}

fn merge_graph(base: GraphConfig, overlay: GraphConfig) -> GraphConfig {
    let default = GraphConfig::default();
    GraphConfig {
        max_depth: if overlay.max_depth != default.max_depth {
            overlay.max_depth
        } else {
            base.max_depth
        },
        decay: if (overlay.decay - default.decay).abs() > 0.001 {
            overlay.decay
        } else {
            base.decay
        },
        path_weight: if (overlay.path_weight - default.path_weight).abs() > 0.001 {
            overlay.path_weight
        } else {
            base.path_weight
        },
        edge_weights: if overlay.edge_weights != default.edge_weights {
            overlay.edge_weights
        } else {
            base.edge_weights
        },
    }
}

fn merge_chunking(base: ChunkingConfig, overlay: ChunkingConfig) -> ChunkingConfig {
    let default = ChunkingConfig::default();
    ChunkingConfig {
        max_tokens: if overlay.max_tokens != default.max_tokens {
            overlay.max_tokens
        } else {
            base.max_tokens
        },
        overlap_tokens: if overlay.overlap_tokens != default.overlap_tokens {
            overlay.overlap_tokens
        } else {
            base.overlap_tokens
        },
        strategy: if overlay.strategy != default.strategy {
            overlay.strategy
        } else {
            base.strategy
        },
        use_cast: if overlay.use_cast != default.use_cast {
            overlay.use_cast
        } else {
            base.use_cast
        },
        max_chars: if overlay.max_chars != default.max_chars {
            overlay.max_chars
        } else {
            base.max_chars
        },
    }
}

fn merge_embedding(base: EmbeddingConfig, overlay: EmbeddingConfig) -> EmbeddingConfig {
    let default = EmbeddingConfig::default();
    EmbeddingConfig {
        backend: if overlay.backend != default.backend {
            overlay.backend
        } else {
            base.backend
        },
        model_name: if overlay.model_name != default.model_name {
            overlay.model_name
        } else {
            base.model_name
        },
    }
}



fn merge_agent(base: AgentConfig, overlay: AgentConfig) -> AgentConfig {
    let default = AgentConfig::default();
    AgentConfig {
        max_per_step: if overlay.max_per_step != default.max_per_step {
            overlay.max_per_step
        } else {
            base.max_per_step
        },
        max_observations: if overlay.max_observations != default.max_observations {
            overlay.max_observations
        } else {
            base.max_observations
        },
        max_tokens: if overlay.max_tokens != default.max_tokens {
            overlay.max_tokens
        } else {
            base.max_tokens
        },

        max_steps: if overlay.max_steps != default.max_steps {
            overlay.max_steps
        } else {
            base.max_steps
        },
        max_total_evidence_lines: if overlay.max_total_evidence_lines
            != default.max_total_evidence_lines
        {
            overlay.max_total_evidence_lines
        } else {
            base.max_total_evidence_lines
        },
        step_timeout_secs: if overlay.step_timeout_secs != default.step_timeout_secs {
            overlay.step_timeout_secs
        } else {
            base.step_timeout_secs
        },
    }
}

fn merge_llm(base: LlmConfig, overlay: LlmConfig) -> LlmConfig {
    let default = LlmConfig::default();
    LlmConfig {
        model: if overlay.model != default.model {
            overlay.model
        } else {
            base.model
        },
        max_tokens: if overlay.max_tokens != default.max_tokens {
            overlay.max_tokens
        } else {
            base.max_tokens
        },
        api_base: overlay.api_base.or(base.api_base),
        timeout_secs: if overlay.timeout_secs != default.timeout_secs {
            overlay.timeout_secs
        } else {
            base.timeout_secs
        },
    }
}

fn merge_core(base: CoreConfig, overlay: CoreConfig) -> CoreConfig {
    let default = CoreConfig::default();
    CoreConfig {
        include_paths: if overlay.include_paths != default.include_paths {
            overlay.include_paths
        } else {
            base.include_paths
        },
        exclude_paths: if !overlay.exclude_paths.is_empty() {
            overlay.exclude_paths
        } else {
            base.exclude_paths
        },
        auto_index_on_search: if overlay.auto_index_on_search != default.auto_index_on_search {
            overlay.auto_index_on_search
        } else {
            base.auto_index_on_search
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_search_overlay_wins() {
        let base = SearchConfig {
            mode: SearchMode::Lexical,
            top_k: 10,
        };
        let overlay = SearchConfig {
            mode: SearchMode::Semantic,
            top_k: 20,
        };
        let merged = merge_search(base, overlay);
        assert_eq!(merged.mode, SearchMode::Semantic);
        assert_eq!(merged.top_k, 20);
    }

    #[test]
    fn test_merge_search_default_overlay_ignored() {
        let base = SearchConfig {
            mode: SearchMode::Semantic,
            top_k: 20,
        };
        let overlay = SearchConfig::default();
        let merged = merge_search(base, overlay);
        assert_eq!(merged.mode, SearchMode::Semantic);
        assert_eq!(merged.top_k, 20);
    }
}
