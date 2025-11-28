//! BM25 algorithm parameters

use serde::{Deserialize, Serialize};

/// BM25 (Best Matching 25) algorithm parameters
///
/// BM25 is a ranking function used for lexical search.
/// These parameters control term frequency saturation and document length normalization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bm25Config {
    /// Term frequency saturation parameter
    ///
    /// Controls how quickly term frequency impact saturates.
    /// Typical range: 1.2 - 2.0
    /// - Lower values (1.2): faster saturation, less emphasis on repeated terms
    /// - Higher values (2.0): slower saturation, more emphasis on repeated terms
    #[serde(default = "default_k1")]
    pub k1: f32,

    /// Document length normalization parameter
    ///
    /// Controls how much document length affects scoring.
    /// Range: 0.0 - 1.0
    /// - 0.0: no length normalization
    /// - 1.0: full length normalization
    /// - 0.75: balanced (recommended for code)
    #[serde(default = "default_b")]
    pub b: f32,

    /// Average document length (in tokens)
    ///
    /// Used for length normalization. This should match your typical code chunk size.
    #[serde(default = "default_avg_len")]
    pub avg_len: usize,
}

impl Default for Bm25Config {
    fn default() -> Self {
        Self {
            k1: default_k1(),
            b: default_b(),
            avg_len: default_avg_len(),
        }
    }
}

impl crate::validation::Validate for Bm25Config {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::validation::{validate_positive, validate_range};

        // k1 should be in reasonable range
        validate_range("bm25.k1", self.k1, 0.5, 3.0)?;

        // b should be [0, 1]
        validate_range("bm25.b", self.b, 0.0, 1.0)?;

        // avg_len must be positive
        validate_positive("bm25.avg_len", self.avg_len, 0)?;

        Ok(())
    }
}

fn default_k1() -> f32 {
    1.2 // Standard BM25 parameter
}

fn default_b() -> f32 {
    0.75 // Standard BM25 parameter
}

fn default_avg_len() -> usize {
    50 // Tokens, roughly matches typical code chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = Bm25Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_k1() {
        let config = Bm25Config {
            k1: 5.0, // Too high
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_b() {
        let config = Bm25Config {
            b: 1.5, // Out of range
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_avg_len_invalid() {
        let config = Bm25Config {
            avg_len: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
