use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, prompt: &str, max_tokens: u32) -> Result<String>;
}

pub struct OpenAiClient {
    model: String,
    api_key: String,
    api_base: String,
    client: Client,
}

impl OpenAiClient {
    pub fn from_env(default_model: &str) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("OPENAI_API_KEY not set; cannot call OpenAI"))?;
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| default_model.to_string());
        let api_base =
            std::env::var("OPENAI_API_BASE").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        Ok(Self {
            model,
            api_key,
            api_base,
            client: Client::new(),
        })
    }

    pub fn from_config(cfg: &coderet_config::LlmConfig) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("OPENAI_API_KEY not set; cannot call OpenAI"))?;
        let api_base = cfg
            .api_base
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        Ok(Self {
            model: cfg.model.clone(),
            api_key,
            api_base,
            client: Client::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        let resp = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.api_base.trim_end_matches('/')
            ))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "max_tokens": max_tokens,
                "temperature": 0.0
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI error ({}): {}", status, body));
        }

        let parsed: ChatResponse = resp.json().await?;
        let content = parsed
            .choices
            .get(0)
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        if content.trim().is_empty() {
            Err(anyhow!("Empty response from OpenAI"))
        } else {
            Ok(content)
        }
    }
}
