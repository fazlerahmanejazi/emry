//! Configuration management for code-retriever
//!
//! This crate provides a robust, validated configuration system with support for:
//! - Multiple formats (YAML, TOML, JSON)
//! - Config validation with helpful error messages
//! - Config merging (file + environment + CLI)
//! - Type-safe configuration structs
//!
//! # Example
//!
//! ```no_run
//! use coderet_config::Config;
//!
//! // Load from default location (.coderet.{yml,toml,json})
//! let config = Config::load()?;
//!
//! // Or load from specific file
//! let config = Config::from_file("path/to/config.toml")?;
//!
//! // Access config values
//! let search_mode = config.search.mode;
//! let chunk_size = config.chunking.max_tokens;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod error;
pub mod loader;
pub mod types;
pub mod validation;

// Re-export main types for convenience
pub use error::{ConfigError, Result};
pub use loader::ConfigBuilder;
pub use types::*;

/// Main configuration struct aggregating all settings
pub use types::Config;

/// Trait for config validation
pub use validation::Validate;
