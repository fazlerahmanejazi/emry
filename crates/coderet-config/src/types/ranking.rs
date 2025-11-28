//! Ranking weights configuration

use serde::{Deserialize, Serialize};

/// Ranking weights for hybrid search
///
/// These weights determine how different scoring signals are combined.
/// All weights should be in [0, 1] and ideally sum to 1.0 for normalized scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    /// Weight for lexical (BM25) scoring
    ///
    /// Higher values prioritize exact keyword matches.
    /// Recommended: 0.4-0.7 for code search
    #[serde(default = "default_lexical")]
    pub lexical: f32,

    /// Weight for vector (semantic) scoring
    ///
    /// Higher values prioritize semantic similarity.
    /// Recommended: 0.3-0.6 for code search
    #[serde(default = "default_vector")]
    pub vector: f32,

    /// Weight for graph-based scoring
    ///
    /// Higher values prioritize code relationships (calls, imports).
    /// Recommended: 0.1-0.3
    #[serde(default = "default_graph")]
    pub graph: f32,

    /// Weight for symbol match boost
    ///
    /// Boost for exact symbol name matches.
    /// Recommended: 0.1-0.2
    /// Weight for symbol match boost
    ///
    /// Boost for exact symbol name matches.
    /// Recommended: 0.1-0.2
    #[serde(default = "default_symbol")]
    pub symbol: f32,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            lexical: default_lexical(),
            vector: default_vector(),
            graph: default_graph(),
            symbol: default_symbol(),
        }
    }
}

impl crate::validation::Validate for RankingConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::validation::{validate_range, validate_weight_sum};

        // Validate each weight is in valid range
        validate_range("ranking.lexical", self.lexical, 0.0, 1.0)?;
        validate_range("ranking.vector", self.vector, 0.0, 1.0)?;
        validate_range("ranking.graph", self.graph, 0.0, 1.0)?;
        validate_range("ranking.symbol", self.symbol, 0.0, 1.0)?;

        // Validate primary weights (lexical + vector) sum to ~1.0
        let weights = vec![
            ("lexical".to_string(), self.lexical),
            ("vector".to_string(), self.vector),
        ];
        validate_weight_sum("ranking (lexical + vector)", &weights, 1.0)?;

        Ok(())
    }
}

// Defaults chosen empirically:
// - Lexical is strong for exact matches (0.6)
// - Vector captures semantic meaning (0.4)
// - Graph and symbol are additive boosts

fn default_lexical() -> f32 {
    0.6 // Emphasize exact matches in code
}

fn default_vector() -> f32 {
    0.4 // Semantic understanding secondary
}

fn default_graph() -> f32 {
    0.2 // Boost for related code
}

fn default_symbol() -> f32 {
    0.15 // Boost for symbol matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = RankingConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_weight_range() {
        let config = RankingConfig {
            lexical: 1.5, // Out of range
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_weight_sum() {
        let config = RankingConfig {
            lexical: 0.3,
            vector: 0.3, // Sum is 0.6, not 1.0
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_custom_valid_weights() {
        let config = RankingConfig {
            lexical: 0.5,
            vector: 0.5,
            graph: 0.15,
            symbol: 0.1,
        };
        assert!(config.validate().is_ok());
    }
}
