//! TOML format parser

use crate::{error::ConfigError, Config, Result};

/// Parse configuration from TOML string
pub fn parse(content: &str) -> Result<Config> {
    parse_with_path(content, None)
}

/// Parse configuration from TOML string with file path for better errors
pub fn parse_with_path(content: &str, path: Option<&str>) -> Result<Config> {
    ::toml::from_str(content).map_err(|e| ConfigError::from_toml_error(e, content, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_toml() {
        let toml = r#"
[search]
mode = "hybrid"
top_k = 5
"#;
        let config = parse(toml).unwrap();
        assert_eq!(config.search.top_k, 5);
    }
}
