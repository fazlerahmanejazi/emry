//! Error types for configuration loading and validation

pub mod format;

use std::path::PathBuf;
use thiserror::Error;

pub use format::ErrorFormatter;

/// Result type for config operations
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Errors that can occur during configuration loading and validation
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Configuration file not found
    #[error("Configuration file not found: {path}")]
    FileNotFound { path: PathBuf },

    /// Unknown configuration format
    #[error("Unknown configuration format for file: {path}\nSupported formats: .yml, .yaml, .toml, .json")]
    UnknownFormat { path: PathBuf },

    /// YAML parsing error with context
    #[error("Failed to parse YAML configuration{location}:\n{message}\n{context}")]
    YamlError {
        location: String,
        message: String,
        context: String,
    },

    /// TOML parsing error with context
    #[error("Failed to parse TOML configuration{location}:\n{message}\n{context}")]
    TomlError {
        location: String,
        message: String,
        context: String,
    },

    /// JSON parsing error with context
    #[error("Failed to parse JSON configuration{location}:\n{message}\n{context}")]
    JsonError {
        location: String,
        message: String,
        context: String,
    },

    /// IO error
    #[error("Failed to read configuration file: {path}\n{source}")]
    IoError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Invalid enum value
    #[error("Invalid value '{value}' for {field}\n  Valid options: {options}\n  Hint: {hint}")]
    InvalidEnum {
        field: String,
        value: String,
        options: String,
        hint: String,
    },

    /// Weight sum validation failed
    #[error("Weight validation failed for {field}\n  Expected sum: {expected}\n  Actual sum: {actual:.3}\n  Hint: {hint}")]
    InvalidWeightSum {
        field: String,
        expected: f32,
        actual: f32,
        hint: String,
    },

    /// Value out of valid range
    #[error("{field} must be between {min} and {max}, got {value}")]
    OutOfRange {
        field: String,
        value: f32,
        min: f32,
        max: f32,
    },

    /// Invalid integer value
    #[error("{field} must be > {min}, got {value}")]
    InvalidInteger {
        field: String,
        value: usize,
        min: usize,
    },

    /// Multiple validation errors
    #[error("Configuration has {count} validation error(s):\n{errors}")]
    MultipleErrors { count: usize, errors: String },

    /// Environment variable parsing error
    #[error("Failed to parse environment variable {var}: {message}")]
    EnvVarError { var: String, message: String },

    /// Config merging error
    #[error("Failed to merge configurations: {message}")]
    MergeError { message: String },

    /// Generic validation error
    #[error("Validation error: {field}: {message}")]
    ValidationError { field: String, message: String },
}

impl ConfigError {
    /// Create an invalid enum error with a suggestion
    pub fn invalid_enum(
        field: impl Into<String>,
        value: impl Into<String>,
        options: &[&str],
    ) -> Self {
        let value = value.into();
        let hint = Self::suggest_option(&value, options);
        Self::InvalidEnum {
            field: field.into(),
            value,
            options: options.join(", "),
            hint,
        }
    }

    /// Create a YAML error from serde_yaml::Error
    pub fn from_yaml_error(err: serde_yaml::Error, content: &str, path: Option<&str>) -> Self {
        let (_location, context) = extract_yaml_context(&err, content);
        Self::YamlError {
            location: path.map(|p| format!(" in {}", p)).unwrap_or_default(),
            message: err.to_string(),
            context,
        }
    }

    /// Create a TOML error from toml::de::Error
    pub fn from_toml_error(err: toml::de::Error, content: &str, path: Option<&str>) -> Self {
        let context = extract_toml_context(&err, content);
        Self::TomlError {
            location: path.map(|p| format!(" in {}", p)).unwrap_or_default(),
            message: err.message().to_string(),
            context,
        }
    }

    /// Create a JSON error from serde_json::Error
    pub fn from_json_error(err: serde_json::Error, content: &str, path: Option<&str>) -> Self {
        let context = extract_json_context(&err, content);
        Self::JsonError {
            location: path.map(|p| format!(" in {}", p)).unwrap_or_default(),
            message: err.to_string(),
            context,
        }
    }

    /// Simple string distance for option suggestions (Levenshtein-like)
    fn suggest_option(input: &str, options: &[&str]) -> String {
        let input_lower = input.to_lowercase();
        let closest = options
            .iter()
            .min_by_key(|opt| Self::distance(&input_lower, &opt.to_lowercase()));

        match closest {
            Some(opt) if Self::distance(&input_lower, &opt.to_lowercase()) <= 3 => {
                format!("Did you mean '{}'?", opt)
            }
            _ => "Check your configuration file".to_string(),
        }
    }

    /// Simple character distance calculation
    fn distance(a: &str, b: &str) -> usize {
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        let mut prev_row: Vec<usize> = (0..=b_chars.len()).collect();

        for (i, a_char) in a_chars.iter().enumerate() {
            let mut curr_row = vec![i + 1];
            for (j, b_char) in b_chars.iter().enumerate() {
                let cost = if a_char == b_char { 0 } else { 1 };
                curr_row.push(
                    *[
                        curr_row[j] + 1,     // insertion
                        prev_row[j + 1] + 1, // deletion
                        prev_row[j] + cost,  // substitution
                    ]
                    .iter()
                    .min()
                    .unwrap(),
                );
            }
            prev_row = curr_row;
        }

        *prev_row.last().unwrap_or(&0)
    }
}

/// Extract context from YAML error
fn extract_yaml_context(err: &serde_yaml::Error, content: &str) -> (String, String) {
    if let Some(loc) = err.location() {
        let line_num = loc.line();
        let lines: Vec<&str> = content.lines().collect();

        if line_num > 0 && line_num <= lines.len() {
            let start = line_num.saturating_sub(2);
            let end = (line_num + 1).min(lines.len());

            let context = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let num = start + i + 1;
                    if num == line_num {
                        format!("→ {:3} | {}", num, line)
                    } else {
                        format!("  {:3} | {}", num, line)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            return (format!(" at line {}", line_num), context);
        }
    }

    (String::new(), String::new())
}

/// Extract context from TOML error
fn extract_toml_context(err: &toml::de::Error, content: &str) -> String {
    if let Some(span) = err.span() {
        let lines: Vec<&str> = content.lines().collect();
        let line_num = content[..span.start].matches('\n').count() + 1;

        if line_num > 0 && line_num <= lines.len() {
            let start = line_num.saturating_sub(2);
            let end = (line_num + 1).min(lines.len());

            return lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let num = start + i + 1;
                    if num == line_num {
                        format!("→ {:3} | {}", num, line)
                    } else {
                        format!("  {:3} | {}", num, line)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }

    String::new()
}

/// Extract context from JSON error
fn extract_json_context(err: &serde_json::Error, content: &str) -> String {
    let line_num = err.line();
    let col_num = err.column();
    let lines: Vec<&str> = content.lines().collect();

    if line_num > 0 && line_num <= lines.len() {
        let start = line_num.saturating_sub(2);
        let end = (line_num + 1).min(lines.len());

        let context = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let num = start + i + 1;
                if num == line_num {
                    let mut result = format!("→ {:3} | {}", num, line);
                    if col_num > 0 {
                        result.push_str(&format!("\n      {}{}", " ".repeat(col_num - 1), "^"));
                    }
                    result
                } else {
                    format!("  {:3} | {}", num, line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        return context;
    }

    String::new()
}
