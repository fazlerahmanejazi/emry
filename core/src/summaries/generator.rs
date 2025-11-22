use anyhow::{anyhow, Result};
use serde_json::json;
use std::env;

pub struct SummaryGenerator {
    client: reqwest::Client,
    api_key: String,
    model: String,
    api_url: String,
}

impl SummaryGenerator {
    pub fn new(model: Option<String>) -> Result<Self> {
        // Check if OpenAI Key is present to decide default provider
        let openai_key = env::var("OPENAI_API_KEY").ok();
        
        let (default_url, default_model, api_key) = if let Some(k) = openai_key {
            // User has OpenAI key, default to OpenAI
            ("https://api.openai.com/v1", "gpt-4o-mini", k)
        } else {
            // No OpenAI key, default to Local Ollama
            ("http://localhost:11434/v1", "qwen2.5-coder:1.5b", "dummy".to_string())
        };

        // Allow overrides via environment variables
        let api_url = env::var("LLM_API_BASE").unwrap_or_else(|_| default_url.to_string());
        let model = model.or_else(|| env::var("LLM_MODEL").ok())
            .unwrap_or_else(|| default_model.to_string());

        // Safety check: If URL is OpenAI but no key provided (and we are using dummy)
        if api_url.contains("openai.com") && api_key == "dummy" {
             return Err(anyhow!("OPENAI_API_KEY environment variable not set for OpenAI URL"));
        }
        
        // Ensure URL ends with /chat/completions if it's just the base
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

    pub fn generate(&self, code: &str, context: &str) -> Result<String> {
        let prompt = format!(
            "You are a code summarization assistant. Summarize the following code concisely.\n\
            Context: {}\n\
            Code:\n```\n{}\n```\n\
            Summary:",
            context, code
        );

        let res = tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                self.client
                    .post(&self.api_url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&json!({
                        "model": self.model,
                        "messages": [
                            {"role": "system", "content": "You are a helpful assistant that summarizes code."},
                            {"role": "user", "content": prompt}
                        ],
                        "max_tokens": 150,
                        "temperature": 0.3
                    }))
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await
            })
        }).map_err(|e| anyhow!("Failed to call OpenAI API: {}", e))?;

        let choices = res.get("choices").ok_or_else(|| anyhow!("Invalid response from OpenAI: {:?}", res))?;
        let choice = choices.get(0).ok_or_else(|| anyhow!("No choices in response"))?;
        let message = choice.get("message").ok_or_else(|| anyhow!("No message in choice"))?;
        let content = message.get("content").ok_or_else(|| anyhow!("No content in message"))?;

        Ok(content.as_str().unwrap_or_default().trim().to_string())
    }
}
