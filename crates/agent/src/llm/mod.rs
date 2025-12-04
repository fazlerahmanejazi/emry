use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn chat(&self, messages: &[Message]) -> Result<String>;
}

pub struct OllamaProvider {
    pub model: String,
    pub base_url: String,
    pub client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(model: String, base_url: String, timeout_secs: u64) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()?;
        Ok(Self {
            model,
            base_url,
            client,
        })
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    async fn chat(&self, messages: &[Message]) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false
        });

        let res = self.client.post(&url).json(&body).send().await?;
        let json: serde_json::Value = res.json().await?;

        Ok(json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string())
    }
}

#[derive(Clone)]
pub struct OpenAIProvider {
    pub model: String,
    pub api_key: String,
    pub client: reqwest::Client,
    pub api_base: String,
}

#[derive(Debug, Clone)]
pub struct JsonSchemaSpec {
    pub name: String,
    pub schema: serde_json::Value,
}

impl OpenAIProvider {
    pub fn new(model: String, api_key: String, timeout_secs: u64) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()?;
        Ok(Self {
            model,
            api_key,
            client,
            api_base: "https://api.openai.com/v1".to_string(),
        })
    }

    pub fn with_base(model: String, api_key: String, api_base: String, timeout_secs: u64) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()?;
        Ok(Self {
            model,
            api_key,
            client,
            api_base,
        })
    }

    async fn chat_inner(
        &self,
        messages: &[Message],
        response_format: Option<JsonSchemaSpec>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages
        });
        if let Some(mt) = max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(spec) = response_format {
            body["response_format"] = serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": spec.name,
                    "schema": spec.schema,
                    "strict": false
                }
            });
        }

        let res = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("OpenAI API error: {} - {}", status, text));
        }

        let json: serde_json::Value = res.json().await?;

        if let Some(error) = json.get("error") {
            return Err(anyhow::anyhow!("OpenAI API returned error: {}", error));
        }

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Invalid response format: missing content in choices"))
    }

    pub async fn chat_with_schema(
        &self,
        messages: &[Message],
        schema: JsonSchemaSpec,
    ) -> Result<String> {
        self.chat_inner(messages, Some(schema), None).await
    }

    pub async fn chat_with_schema_and_limit(
        &self,
        messages: &[Message],
        schema: JsonSchemaSpec,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        self.chat_inner(messages, Some(schema), max_tokens).await
    }

    pub async fn chat_with_limit(
        &self,
        messages: &[Message],
        max_tokens: Option<u32>,
    ) -> Result<String> {
        self.chat_inner(messages, None, max_tokens).await
    }
}

#[async_trait]
impl ModelProvider for OpenAIProvider {
    async fn chat(&self, messages: &[Message]) -> Result<String> {
        self.chat_inner(messages, None, None).await
    }
}

#[async_trait]
impl emry_core::traits::LLM for OpenAIProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        self.chat(&[Message {
            role: "user".to_string(),
            content: prompt.to_string(),
        }]).await
    }
}
