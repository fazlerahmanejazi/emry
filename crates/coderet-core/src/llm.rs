use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;

pub trait LlmClient {
    fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String>;
}

pub struct OpenAiClient {
    api_key: String,
    model: String,
    client: Client,
}

impl OpenAiClient {
    pub fn new(model: String, api_key: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatRespChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResp {
    choices: Vec<ChatRespChoice>,
}

impl LlmClient for OpenAiClient {
    fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        let url = "https://api.openai.com/v1/chat/completions";
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "max_tokens": max_tokens,
        });
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()?;
        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(anyhow::anyhow!("OpenAI error {}: {}", status, txt));
        }
        let parsed: ChatResp = resp.json()?;
        let content = parsed
            .choices
            .get(0)
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        Ok(content)
    }
}

pub struct OllamaClient {
    model: String,
    base_url: String,
    client: Client,
}

impl OllamaClient {
    pub fn new(model: String, base_url: Option<String>) -> Self {
        Self {
            model,
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
            client: Client::new(),
        }
    }
}

impl LlmClient for OllamaClient {
    fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "options": { "num_predict": max_tokens as i64 }
        });
        let resp = self.client.post(url).json(&body).send()?;
        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(anyhow::anyhow!("Ollama error {}: {}", status, txt));
        }
        let json: serde_json::Value = resp.json()?;
        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(content)
    }
}
