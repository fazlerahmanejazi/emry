//! Enhanced error formatting with colors and context

use crate::error::ConfigError;
use std::fmt;
use yansi::Paint;

/// Format error with colors and context
pub struct ErrorFormatter {
    error: ConfigError,
    use_colors: bool,
}

impl ErrorFormatter {
    /// Create a new error formatter
    pub fn new(error: ConfigError) -> Self {
        Self {
            error,
            use_colors: supports_color(),
        }
    }

    /// Format the error with colors and context
    pub fn format(&self) -> String {
        if self.use_colors {
            self.format_colored()
        } else {
            self.format_plain()
        }
    }

    fn format_colored(&self) -> String {
        match &self.error {
            ConfigError::InvalidEnum {
                field,
                value,
                options,
                hint,
            } => {
                let value_str = format!("'{}'", value);
                format!(
                    "{} Invalid value {} for {}\n  {}: {}\n  {}: {}",
                    Paint::red("✗").bold(),
                    Paint::yellow(&value_str),
                    Paint::cyan(field),
                    Paint::new("Valid options").bold(),
                    options,
                    Paint::new("Hint").bold(),
                    Paint::green(hint)
                )
            }
            ConfigError::InvalidWeightSum {
                field,
                expected,
                actual,
                hint,
            } => {
                let actual_str = format!("{:.3}", actual);
                format!(
                    "{} Weight validation failed for {}\n  Expected sum: {}\n  Actual sum:  {}\n  {}: {}",
                    Paint::red("✗").bold(),
                    Paint::cyan(field),
                    Paint::green(expected),
                    Paint::yellow(&actual_str),
                    Paint::new("Hint").bold(),
                    hint
                )
            }
            ConfigError::OutOfRange {
                field,
                value,
                min,
                max,
            } => {
                let value_str = format!("{}", value);
                format!(
                    "{} {} must be between {} and {}, got {}",
                    Paint::red("✗").bold(),
                    Paint::cyan(field),
                    Paint::green(min),
                    Paint::green(max),
                    Paint::red(&value_str)
                )
            }
            ConfigError::ValidationError { field, message } => {
                format!(
                    "{} {}: {}",
                    Paint::red("✗").bold(),
                    Paint::cyan(field),
                    message
                )
            }
            ConfigError::FileNotFound { path } => {
                let path_str = path.display().to_string();
                format!(
                    "{} Configuration file not found: {}",
                    Paint::red("✗").bold(),
                    Paint::yellow(&path_str)
                )
            }
            _ => self.format_plain(),
        }
    }

    fn format_plain(&self) -> String {
        self.error.to_string()
    }
}

/// Check if terminal supports colors
fn supports_color() -> bool {
    // Check if NO_COLOR is set
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }

    // Check if we're in a TTY
    atty::is(atty::Stream::Stderr)
}

impl fmt::Display for ErrorFormatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_invalid_enum() {
        let error =
            ConfigError::invalid_enum("search.mode", "foo", &["lexical", "semantic", "hybrid"]);
        let formatter = ErrorFormatter {
            error,
            use_colors: false,
        };
        let output = formatter.format();
        assert!(output.contains("Invalid value"));
        assert!(output.contains("'foo'"));
    }

    #[test]
    fn test_supports_color() {
        // Just test that it doesn't panic
        let _ = supports_color();
    }
}
