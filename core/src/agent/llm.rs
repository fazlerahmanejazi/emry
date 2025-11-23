use anyhow::{anyhow, Result};
use serde_json::json;
use std::env;

#[derive(Clone)]
pub struct LLMClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    api_url: String,
}

impl LLMClient {
    pub fn new(model: Option<String>) -> Result<Self> {
        let openai_key = env::var("OPENAI_API_KEY").ok();

        let (default_url, default_model, api_key) = if let Some(k) = openai_key {
            ("https://api.openai.com/v1", "gpt-4o-mini", k)
        } else {
            (
                "http://localhost:11434/v1",
                "qwen2.5-coder:1.5b",
                "dummy".to_string(),
            )
        };

        let api_url = env::var("LLM_API_BASE").unwrap_or_else(|_| default_url.to_string());
        let model = model
            .or_else(|| env::var("LLM_MODEL").ok())
            .unwrap_or_else(|| default_model.to_string());

        if api_url.contains("openai.com") && api_key == "dummy" {
            return Err(anyhow!(
                "OPENAI_API_KEY environment variable not set for OpenAI URL"
            ));
        }

        let endpoint = if api_url.ends_with("/chat/completions") {
            api_url
        } else {
            format!("{}/chat/completions", api_url.trim_end_matches('/'))
        };

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            api_url: endpoint,
        })
    }

    pub async fn chat(&self, messages: Vec<serde_json::Value>) -> Result<String> {
        let res = self.client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "model": self.model,
                "messages": messages,
                "temperature": 0.0 // Deterministic for agent
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let choices = res
            .get("choices")
            .ok_or_else(|| anyhow!("Invalid response from LLM: {:?}", res))?;
        let choice = choices
            .get(0)
            .ok_or_else(|| anyhow!("No choices in response"))?;
        let message = choice
            .get("message")
            .ok_or_else(|| anyhow!("No message in choice"))?;
        let content = message
            .get("content")
            .ok_or_else(|| anyhow!("No content in message"))?;

        Ok(content.as_str().unwrap_or_default().trim().to_string())
    }
}
