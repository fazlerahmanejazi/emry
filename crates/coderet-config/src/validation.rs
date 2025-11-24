//! Validation trait and implementations for configuration types

use crate::error::{ConfigError, Result};

/// Trait for validating configuration values
///
/// Implement this trait for any config type that needs validation beyond
/// type-level checks. Validation should be comprehensive and provide helpful
/// error messages.
pub trait Validate {
    /// Validate the configuration
    ///
    /// Returns `Ok(())` if validation passes, or a `ConfigError` describing
    /// what validation failed and why.
    fn validate(&self) -> Result<()>;
}

/// Helper function to validate weight sums
///
/// Checks that a collection of weights sums to approximately 1.0 (within epsilon).
pub fn validate_weight_sum(
    field: impl Into<String>,
    weights: &[(String, f32)],
    expected: f32,
) -> Result<()> {
    let sum: f32 = weights.iter().map(|(_, w)| w).sum();
    let epsilon = 0.01; // Allow 1% tolerance

    if (sum - expected).abs() > epsilon {
        return Err(ConfigError::InvalidWeightSum {
            field: field.into(),
            expected,
            actual: sum,
            hint: format!(
                "Adjust the following weights: {}",
                weights
                    .iter()
                    .map(|(name, val)| format!("{} = {:.2}", name, val))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    Ok(())
}

/// Helper function to validate value is within range
pub fn validate_range(field: impl Into<String>, value: f32, min: f32, max: f32) -> Result<()> {
    if !(min..=max).contains(&value) {
        return Err(ConfigError::OutOfRange {
            field: field.into(),
            value,
            min,
            max,
        });
    }
    Ok(())
}

/// Helper function to validate integer is above minimum
pub fn validate_positive(field: impl Into<String>, value: usize, min: usize) -> Result<()> {
    if value <= min {
        return Err(ConfigError::InvalidInteger {
            field: field.into(),
            value,
            min,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weight_sum_valid() {
        let weights = vec![
            ("a".to_string(), 0.5),
            ("b".to_string(), 0.3),
            ("c".to_string(), 0.2),
        ];
        assert!(validate_weight_sum("test", &weights, 1.0).is_ok());
    }

    #[test]
    fn test_weight_sum_invalid() {
        let weights = vec![("a".to_string(), 0.5), ("b".to_string(), 0.3)];
        assert!(validate_weight_sum("test", &weights, 1.0).is_err());
    }

    #[test]
    fn test_range_valid() {
        assert!(validate_range("test", 0.5, 0.0, 1.0).is_ok());
    }

    #[test]
    fn test_range_invalid() {
        assert!(validate_range("test", 1.5, 0.0, 1.0).is_err());
    }

    #[test]
    fn test_positive_valid() {
        assert!(validate_positive("test", 5, 0).is_ok());
    }

    #[test]
    fn test_positive_invalid() {
        assert!(validate_positive("test", 0, 0).is_err());
    }
}
