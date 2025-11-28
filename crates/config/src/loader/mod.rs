//! Configuration loading from various sources

pub mod env;
pub mod file;
pub mod formats;
pub mod merge;

use crate::{Config, Result, Validate};
use std::path::{Path, PathBuf};

/// Format for configuration files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// YAML format (.yml, .yaml)
    Yaml,
    /// TOML format (.toml)
    Toml,
    /// JSON format (.json)
    Json,
}

/// Configuration source for layered loading
#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// Load from a file
    File(PathBuf),
    /// Load from environment variables
    Environment,
    /// Explicit config object (for programmatic use)
    Explicit(Config),
}

/// Builder for loading and merging configurations
///
/// Supports layered configuration with proper precedence:
/// defaults < file < environment < explicit overrides
///
/// # Example
///
/// ```no_run
/// use emry_config::loader::ConfigBuilder;
///
/// let config = ConfigBuilder::new()
///     .with_file(".emry.toml")      // Base config
///     .with_env()                       // Env var overlay
///     .build()?;
/// # Ok::<(), emry_config::error::ConfigError>(())
/// ```
pub struct ConfigBuilder {
    sources: Vec<ConfigSource>,
}

impl ConfigBuilder {
    /// Create a new config builder starting with defaults
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Add a file source
    pub fn with_file<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.sources
            .push(ConfigSource::File(path.as_ref().to_path_buf()));
        self
    }

    /// Add environment variable overlay
    pub fn with_env(mut self) -> Self {
        self.sources.push(ConfigSource::Environment);
        self
    }

    /// Add explicit config overlay (for programmatic use)
    pub fn with_config(mut self, config: Config) -> Self {
        self.sources.push(ConfigSource::Explicit(config));
        self
    }

    /// Build and validate the final configuration
    ///
    /// Merges all sources in order, with later sources taking precedence.
    pub fn build(self) -> Result<Config> {
        let mut config = Config::default();

        for source in self.sources {
            match source {
                ConfigSource::File(path) => {
                    let file_config = file::load_from_file(&path)?;
                    config = merge::merge(config, file_config);
                }
                ConfigSource::Environment => {
                    if let Some(env_config) = env::from_env()? {
                        config = merge::merge(config, env_config);
                    }
                }
                ConfigSource::Explicit(explicit_config) => {
                    config = merge::merge(config, explicit_config);
                }
            }
        }

        // Final validation
        config.validate()?;
        Ok(config)
    }

    /// Load from a single file (convenience method)
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
        Self::new().with_file(path).build()
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    /// Load configuration from default locations
    ///
    /// Searches for configuration in the following order:
    /// 1. `.emry.toml`
    /// 2. `.emry.yml` or `.emry.yaml`
    /// 3. `.emry.json`
    /// 4. `.code-retriever.yml` (legacy)
    ///
    /// If no file is found, returns default configuration.
    /// Also applies environment variable overlays.
    pub fn load() -> Result<Self> {
        // Try each default location
        let default_paths = [
            ".emry.toml",
            ".emry.yml",
            ".emry.yaml",
            ".emry.json",
            ".code-retriever.yml", // Legacy support
        ];

        let mut builder = ConfigBuilder::new();

        // Find first existing file
        for path in &default_paths {
            if Path::new(path).exists() {
                builder = builder.with_file(path);
                break;
            }
        }

        // Always apply env var overlay
        builder = builder.with_env();

        builder.build()
    }

    /// Load configuration from a specific file
    ///
    /// Also applies environment variable overlays.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        ConfigBuilder::new().with_file(path).with_env().build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_load_defaults() {
        let config = Config::load().unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_builder_default() {
        let config = ConfigBuilder::new().build().unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_builder_with_env() {
        env::set_var("CODERET_SEARCH_TOP_K", "25");
        let config = ConfigBuilder::new().with_env().build().unwrap();
        assert_eq!(config.search.top_k, 25);
        env::remove_var("CODERET_SEARCH_TOP_K");
    }
}
