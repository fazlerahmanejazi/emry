//! Agent configuration - SINGLE SOURCE OF TRUTH

use serde::{Deserialize, Serialize};

/// Agent behavior limits and budgets
///
/// This is the ONLY definition of AgentConfig.
/// All crates should import from coderet-config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Max results per tool call
    ///
    /// Limits how many results each search/retrieval operation returns.
    #[serde(default = "default_max_per_step")]
    pub max_per_step: usize,

    /// Max observations fed to synthesizer
    ///
    /// Limits total observations to prevent context overflow.
    #[serde(default = "default_max_observations")]
    pub max_observations: usize,

    /// Max tokens for LLM calls
    ///
    /// Token budget for agent LLM requests.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,



    /// Max plan steps to execute
    ///
    /// Maximum number of agent steps before forcing termination.
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,

    /// Max total evidence lines collected
    ///
    /// Total line budget across all evidence gathering.
    #[serde(default = "default_max_total_lines")]
    pub max_total_evidence_lines: usize,

    /// Timeout per step (seconds)
    ///
    /// Time limit for each agent step (best-effort).
    #[serde(default = "default_step_timeout")]
    pub step_timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_per_step: default_max_per_step(),
            max_observations: default_max_observations(),
            max_tokens: default_max_tokens(),

            max_steps: default_max_steps(),
            max_total_evidence_lines: default_max_total_lines(),
            step_timeout_secs: default_step_timeout(),
        }
    }
}

impl crate::validation::Validate for AgentConfig {
    fn validate(&self) -> crate::error::Result<()> {
        use crate::validation::validate_positive;

        validate_positive("agent.max_per_step", self.max_per_step, 0)?;
        validate_positive("agent.max_observations", self.max_observations, 0)?;
        validate_positive("agent.max_steps", self.max_steps, 0)?;
        validate_positive(
            "agent.max_total_evidence_lines",
            self.max_total_evidence_lines,
            0,
        )?;

        if self.max_tokens == 0 {
            return Err(crate::error::ConfigError::ValidationError {
                field: "agent.max_tokens".to_string(),
                message: "max_tokens must be > 0".to_string(),
            });
        }

        if self.step_timeout_secs == 0 {
            return Err(crate::error::ConfigError::ValidationError {
                field: "agent.step_timeout_secs".to_string(),
                message: "step_timeout_secs must be > 0".to_string(),
            });
        }

        Ok(())
    }
}

fn default_max_per_step() -> usize {
    6
}

fn default_max_observations() -> usize {
    10
}

fn default_max_tokens() -> u32 {
    800
}



fn default_max_steps() -> usize {
    12
}

fn default_max_total_lines() -> usize {
    600
}

fn default_step_timeout() -> u64 {
    30
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_default_is_valid() {
        let config = AgentConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_zero_max_steps_invalid() {
        let config = AgentConfig {
            max_steps: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_max_tokens_invalid() {
        let config = AgentConfig {
            max_tokens: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
