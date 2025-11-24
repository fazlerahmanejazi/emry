//! File-based configuration loading

use crate::{error::ConfigError, loader::ConfigFormat, Config, Result, Validate};
use std::fs;
use std::path::Path;

/// Load configuration from a file
pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();

    // Detect format from extension
    let format = detect_format(path)?;

    // Read file contents
    let content = fs::read_to_string(path).map_err(|e| ConfigError::IoError {
        path: path.to_path_buf(),
        source: e,
    })?;

    // Get path string for error messages
    let path_str = path.to_str();

    // Parse based on format
    let config = match format {
        ConfigFormat::Yaml => super::formats::yaml::parse_with_path(&content, path_str)?,
        ConfigFormat::Toml => super::formats::toml::parse_with_path(&content, path_str)?,
        ConfigFormat::Json => super::formats::json::parse_with_path(&content, path_str)?,
    };

    // Validate before returning
    config.validate()?;

    Ok(config)
}

/// Detect configuration format from file extension
fn detect_format(path: &Path) -> Result<ConfigFormat> {
    match path.extension().and_then(|s| s.to_str()) {
        Some("yml") | Some("yaml") => Ok(ConfigFormat::Yaml),
        Some("toml") => Ok(ConfigFormat::Toml),
        Some("json") => Ok(ConfigFormat::Json),
        _ => Err(ConfigError::UnknownFormat {
            path: path.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_yaml() {
        assert_eq!(
            detect_format(&PathBuf::from("config.yml")).unwrap(),
            ConfigFormat::Yaml
        );
        assert_eq!(
            detect_format(&PathBuf::from("config.yaml")).unwrap(),
            ConfigFormat::Yaml
        );
    }

    #[test]
    fn test_detect_toml() {
        assert_eq!(
            detect_format(&PathBuf::from("config.toml")).unwrap(),
            ConfigFormat::Toml
        );
    }

    #[test]
    fn test_detect_json() {
        assert_eq!(
            detect_format(&PathBuf::from("config.json")).unwrap(),
            ConfigFormat::Json
        );
    }

    #[test]
    fn test_unknown_format() {
        assert!(detect_format(&PathBuf::from("config.txt")).is_err());
    }
}
