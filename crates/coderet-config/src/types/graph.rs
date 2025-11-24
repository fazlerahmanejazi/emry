//! Graph traversal configuration

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Graph traversal and scoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    /// Maximum depth for graph traversal
    ///
    /// Controls how many hops to explore from seed nodes.
    /// Recommended: 2-5
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Score decay factor per hop
    ///
    /// Applied multiplicatively at each graph hop.
    /// Range: 0.1 - 1.0
    /// - 1.0: no decay
    /// - 0.5: half score each hop
    #[serde(default = "default_decay")]
    pub decay: f32,

    /// Weight for path-based results
    ///
    /// Multiplier for scores of nodes found via graph traversal.
    #[serde(default = "default_path_weight")]
    pub path_weight: f32,

    /// Edge type weights
    ///
    /// Different edge types can have different traversal costs.
    /// Keys: "calls", "imports", "defines", "contains"
    #[serde(default = "default_edge_weights")]
    pub edge_weights: HashMap<String, f32>,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            decay: default_decay(),
            path_weight: default_path_weight(),
            edge_weights: default_edge_weights(),
        }
    }
}

impl crate::validation::Validate for GraphConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;
        use crate::validation::{validate_positive, validate_range};

        // max_depth must be reasonable
        validate_positive("graph.max_depth", self.max_depth, 0)?;
        if self.max_depth > 10 {
            return Err(ConfigError::ValidationError {
                field: "graph.max_depth".to_string(),
                message: format!(
                    "max_depth too large ({}), consider using <= 10",
                    self.max_depth
                ),
            });
        }

        // decay must be in valid range
        validate_range("graph.decay", self.decay, 0.1, 1.0)?;

        // path_weight must be positive
        validate_range("graph.path_weight", self.path_weight, 0.0, 2.0)?;

        // Validate edge weights
        for (edge_type, weight) in &self.edge_weights {
            validate_range(
                &format!("graph.edge_weights.{}", edge_type),
                *weight,
                0.0,
                2.0,
            )?;
        }

        Ok(())
    }
}

fn default_max_depth() -> usize {
    4 // Balance between coverage and performance
}

fn default_decay() -> f32 {
    0.5 // Half score each hop
}

fn default_path_weight() -> f32 {
    1.0 // Equal weight to direct matches
}

fn default_edge_weights() -> HashMap<String, f32> {
    let mut weights = HashMap::new();
    weights.insert("defines".to_string(), 1.25); // Strong relationship
    weights.insert("calls".to_string(), 1.0); // Standard relationship
    weights.insert("imports".to_string(), 0.75); // Weaker relationship
    weights.insert("contains".to_string(), 0.6); // Structural relationship
    weights
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = GraphConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_max_depth() {
        let config = GraphConfig {
            max_depth: 20, // Too deep
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_decay() {
        let config = GraphConfig {
            decay: 1.5, // Out of range
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
