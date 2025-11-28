//! Environment variable configuration overlay
//!
//! Supports environment variables in the format:
//! `CODERET_<section>_<field>=value`
//!
//! Examples:
//! - `CODERET_SEARCH_MODE=semantic`
//! - `CODERET_SEARCH_TOP_K=20`
//! - `CODERET_CHUNKING_MAX_TOKENS=1024`

use crate::{error::ConfigError, types::*, Config, Result};
use std::env;

/// Parse configuration from environment variables
pub fn from_env() -> Result<Option<Config>> {
    let mut config = Config::default();
    let mut found_any = false;

    // Collect all CODERET_ env vars
    let env_vars: Vec<(String, String)> = env::vars()
        .filter(|(k, _)| k.starts_with("CODERET_"))
        .collect();

    if env_vars.is_empty() {
        return Ok(None);
    }

    for (key, value) in env_vars {
        found_any = true;
        if let Err(e) = apply_env_var(&mut config, &key, &value) {
            eprintln!("Warning: failed to parse {}: {}", key, e);
        }
    }

    if found_any {
        Ok(Some(config))
    } else {
        Ok(None)
    }
}

/// Apply a single environment variable to config
fn apply_env_var(config: &mut Config, key: &str, value: &str) -> Result<()> {
    // Strip CODERET_ prefix
    let key = key.strip_prefix("CODERET_").unwrap_or(key);

    // Split into section and field
    let parts: Vec<&str> = key.split('_').collect();
    if parts.len() < 2 {
        return Err(ConfigError::EnvVarError {
            var: key.to_string(),
            message: "Expected format: CODERET_<section>_<field>".to_string(),
        });
    }

    let section = parts[0].to_lowercase();
    let field = parts[1..].join("_").to_lowercase();

    match section.as_str() {
        "search" => apply_search_var(&mut config.search, &field, value),
        "ranking" => apply_ranking_var(&mut config.ranking, &field, value),
        "chunking" => apply_chunking_var(&mut config.chunking, &field, value),
        "embedding" => apply_embedding_var(&mut config.embedding, &field, value),
        "agent" => apply_agent_var(&mut config.agent, &field, value),
        "llm" => apply_llm_var(&mut config.llm, &field, value),
        "bm25" => apply_bm25_var(&mut config.bm25, &field, value),
        "graph" => apply_graph_var(&mut config.graph, &field, value),
        _ => Err(ConfigError::EnvVarError {
            var: key.to_string(),
            message: format!("Unknown section: {}", section),
        }),
    }
}

fn apply_search_var(config: &mut SearchConfig, field: &str, value: &str) -> Result<()> {
    match field {
        "mode" => {
            config.mode = match value.to_lowercase().as_str() {
                "lexical" => SearchMode::Lexical,
                "semantic" => SearchMode::Semantic,
                "hybrid" => SearchMode::Hybrid,
                _ => {
                    return Err(ConfigError::invalid_enum(
                        "search.mode",
                        value,
                        &["lexical", "semantic", "hybrid"],
                    ))
                }
            };
        }
        "top_k" => {
            config.top_k = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_SEARCH_TOP_K".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_SEARCH_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn apply_ranking_var(config: &mut RankingConfig, field: &str, value: &str) -> Result<()> {
    let parse_float = |v: &str| -> Result<f32> {
        v.parse().map_err(|_| ConfigError::EnvVarError {
            var: format!("CODERET_RANKING_{}", field.to_uppercase()),
            message: format!("Invalid float: {}", v),
        })
    };

    match field {
        "lexical" => config.lexical = parse_float(value)?,
        "vector" => config.vector = parse_float(value)?,
        "graph" => config.graph = parse_float(value)?,
        "symbol" => config.symbol = parse_float(value)?,
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_RANKING_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn apply_chunking_var(config: &mut ChunkingConfig, field: &str, value: &str) -> Result<()> {
    match field {
        "max_tokens" => {
            config.max_tokens = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_CHUNKING_MAX_TOKENS".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        "overlap_tokens" => {
            config.overlap_tokens = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_CHUNKING_OVERLAP_TOKENS".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        "strategy" => {
            config.strategy = match value.to_lowercase().as_str() {
                "truncate" => SplitStrategy::Truncate,
                "split" => SplitStrategy::Split,
                "hierarchical" => SplitStrategy::Hierarchical,
                _ => {
                    return Err(ConfigError::invalid_enum(
                        "chunking.strategy",
                        value,
                        &["truncate", "split", "hierarchical"],
                    ))
                }
            };
        }
        "use_cast" => {
            config.use_cast = parse_bool(value)?;
        }
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_CHUNKING_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn apply_embedding_var(config: &mut EmbeddingConfig, field: &str, value: &str) -> Result<()> {
    match field {
        "backend" => {
            config.backend = match value.to_lowercase().as_str() {
                "openai" | "external" => EmbeddingBackend::External,
                "ollama" => EmbeddingBackend::Ollama,
                "local" => EmbeddingBackend::Local,
                _ => {
                    return Err(ConfigError::invalid_enum(
                        "embedding.backend",
                        value,
                        &["openai", "ollama", "local"],
                    ))
                }
            };
        }
        "model_name" => {
            config.model_name = value.to_string();
        }
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_EMBEDDING_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}



fn apply_agent_var(config: &mut AgentConfig, field: &str, value: &str) -> Result<()> {
    match field {
        "max_per_step" => {
            config.max_per_step = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_AGENT_MAX_PER_STEP".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        "max_steps" => {
            config.max_steps = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_AGENT_MAX_STEPS".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        "max_tokens" => {
            config.max_tokens = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_AGENT_MAX_TOKENS".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_AGENT_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn apply_llm_var(config: &mut LlmConfig, field: &str, value: &str) -> Result<()> {
    match field {
        "model" => config.model = value.to_string(),
        "max_tokens" => {
            config.max_tokens = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_LLM_MAX_TOKENS".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        "api_base" => config.api_base = Some(value.to_string()),
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_LLM_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn apply_bm25_var(config: &mut Bm25Config, field: &str, value: &str) -> Result<()> {
    let parse_float = |v: &str| -> Result<f32> {
        v.parse().map_err(|_| ConfigError::EnvVarError {
            var: format!("CODERET_BM25_{}", field.to_uppercase()),
            message: format!("Invalid float: {}", v),
        })
    };

    match field {
        "k1" => config.k1 = parse_float(value)?,
        "b" => config.b = parse_float(value)?,
        "avg_len" => {
            config.avg_len = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_BM25_AVG_LEN".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_BM25_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn apply_graph_var(config: &mut GraphConfig, field: &str, value: &str) -> Result<()> {
    let parse_float = |v: &str| -> Result<f32> {
        v.parse().map_err(|_| ConfigError::EnvVarError {
            var: format!("CODERET_GRAPH_{}", field.to_uppercase()),
            message: format!("Invalid float: {}", v),
        })
    };

    match field {
        "max_depth" => {
            config.max_depth = value.parse().map_err(|_| ConfigError::EnvVarError {
                var: "CODERET_GRAPH_MAX_DEPTH".to_string(),
                message: format!("Invalid integer: {}", value),
            })?;
        }
        "decay" => config.decay = parse_float(value)?,
        "path_weight" => config.path_weight = parse_float(value)?,
        _ => {
            return Err(ConfigError::EnvVarError {
                var: format!("CODERET_GRAPH_{}", field.to_uppercase()),
                message: format!("Unknown field: {}", field),
            })
        }
    }
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(ConfigError::EnvVarError {
            var: value.to_string(),
            message: format!(
                "Invalid boolean: {} (use true/false, 1/0, yes/no, on/off)",
                value
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // Global lock to serialize env var tests
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn cleanup_emry_env_vars() {
        let keys: Vec<String> = env::vars()
            .filter(|(k, _)| k.starts_with("CODERET_"))
            .map(|(k, _)| k)
            .collect();
        for key in keys {
            env::remove_var(&key);
        }
    }

    #[test]
    fn test_search_mode_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        cleanup_emry_env_vars();
        env::set_var("CODERET_SEARCH_MODE", "semantic");
        let config = from_env().unwrap().unwrap();
        assert_eq!(config.search.mode, SearchMode::Semantic);
        cleanup_emry_env_vars();
    }

    #[test]
    fn test_chunking_max_tokens_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        cleanup_emry_env_vars();
        env::set_var("CODERET_CHUNKING_MAX_TOKENS", "1024");
        let config = from_env().unwrap().unwrap();
        assert_eq!(config.chunking.max_tokens, 1024);
        cleanup_emry_env_vars();
    }

    #[test]
    fn test_bool_parsing() {
        // No env vars needed
        assert_eq!(parse_bool("true").unwrap(), true);
        assert_eq!(parse_bool("1").unwrap(), true);
        assert_eq!(parse_bool("yes").unwrap(), true);
        assert_eq!(parse_bool("false").unwrap(), false);
        assert_eq!(parse_bool("0").unwrap(), false);
        assert!(parse_bool("invalid").is_err());
    }

    #[test]
    fn test_no_env_vars() {
        let _lock = ENV_LOCK.lock().unwrap();
        cleanup_emry_env_vars();
        assert!(from_env().unwrap().is_none());
    }
}
