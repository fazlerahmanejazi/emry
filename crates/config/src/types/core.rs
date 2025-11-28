//! Core configuration (paths, storage, file scanning)

use serde::{Deserialize, Serialize};

/// Core configuration for file scanning and storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// Glob patterns for files to include in indexing
    ///
    /// Examples: `["**/*.rs", "**/*.py"]`
    #[serde(default = "default_include_paths")]
    pub include_paths: Vec<String>,

    /// Glob patterns for files to exclude from indexing
    ///
    /// Examples: `["**/target/**", "**/.git/**"]`
    #[serde(default)]
    pub exclude_paths: Vec<String>,

    /// Automatically index on search if index is stale
    #[serde(default = "default_auto_index")]
    pub auto_index_on_search: bool,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            include_paths: default_include_paths(),
            exclude_paths: vec![],
            auto_index_on_search: default_auto_index(),
        }
    }
}

impl crate::validation::Validate for CoreConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::error::ConfigError;

        if self.include_paths.is_empty() {
            return Err(ConfigError::ValidationError {
                field: "core.include_paths".to_string(),
                message: "Must have at least one include pattern".to_string(),
            });
        }

        // Validate glob patterns are valid
        for pattern in &self.include_paths {
            if pattern.is_empty() {
                return Err(ConfigError::ValidationError {
                    field: "core.include_paths".to_string(),
                    message: "Include patterns cannot be empty strings".to_string(),
                });
            }
        }

        Ok(())
    }
}

fn default_include_paths() -> Vec<String> {
    vec!["**/*".to_string()]
}

fn default_auto_index() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_config_is_valid() {
        let config = CoreConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_include_paths_invalid() {
        let config = CoreConfig {
            include_paths: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = CoreConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let deserialized: CoreConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.include_paths, deserialized.include_paths);
    }
}
