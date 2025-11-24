//! JSON format parser

use crate::{error::ConfigError, Config, Result};

/// Parse configuration from JSON string
pub fn parse(content: &str) -> Result<Config> {
    parse_with_path(content, None)
}

/// Parse configuration from JSON string with file path for better errors
pub fn parse_with_path(content: &str, path: Option<&str>) -> Result<Config> {
    serde_json::from_str(content).map_err(|e| ConfigError::from_json_error(e, content, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_json() {
        let json = r#" {
            "search": {
                "mode": "hybrid",
                "top_k": 5
            }
        }"#;
        let config = parse(json).unwrap();
        assert_eq!(config.search.top_k, 5);
    }
}
