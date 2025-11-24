//! YAML format parser

use crate::{error::ConfigError, Config, Result};

/// Parse configuration from YAML string
pub fn parse(content: &str) -> Result<Config> {
    parse_with_path(content, None)
}

/// Parse configuration from YAML string with file path for better errors
pub fn parse_with_path(content: &str, path: Option<&str>) -> Result<Config> {
    serde_yaml::from_str(content).map_err(|e| ConfigError::from_yaml_error(e, content, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_yaml() {
        let yaml = r#"
search:
  mode: hybrid
  top_k: 5
"#;
        let config = parse(yaml).unwrap();
        assert_eq!(config.search.top_k, 5);
    }

    #[test]
    fn test_parse_empty_yaml() {
        let yaml = "{}";
        let config = parse(yaml).unwrap();
        // Should use defaults
        assert_eq!(config.search.top_k, 10);
    }

    #[test]
    fn test_parse_invalid_yaml_shows_line() {
        let yaml = r#"
search:
  mode: invalid_mode
"#;
        let result = parse(yaml);
        assert!(result.is_err());
        // Error should contain context
    }
}
