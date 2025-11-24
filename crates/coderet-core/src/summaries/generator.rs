use anyhow::{anyhow, Result};
use serde_json::json;
use std::env;

#[derive(Clone)]
pub struct SummaryGenerator {
    client: reqwest::Client,
    api_key: String,
    model: String,
    api_url: String,
    pub max_tokens: usize,
    pub prompt_version: String,
    pub retries: u8,
}

impl SummaryGenerator {
    pub fn new(
        model: Option<String>,
        max_tokens: usize,
        prompt_version: String,
        retries: u8,
    ) -> Result<Self> {
        // Check if OpenAI Key is present to decide default provider
        let openai_key = env::var("OPENAI_API_KEY").ok();

        let (default_url, default_model, api_key) = if let Some(k) = openai_key {
            // User has OpenAI key, default to OpenAI
            ("https://api.openai.com/v1", "gpt-4o-mini", k)
        } else {
            // No OpenAI key, default to Local Ollama
            (
                "http://localhost:11434/v1",
                "qwen2.5-coder:1.5b",
                "dummy".to_string(),
            )
        };

        // Allow overrides via environment variables
        let api_url = env::var("LLM_API_BASE").unwrap_or_else(|_| default_url.to_string());
        let model = model
            .or_else(|| env::var("LLM_MODEL").ok())
            .unwrap_or_else(|| default_model.to_string());

        // Safety check: If URL is OpenAI but no key provided (and we are using dummy)
        if api_url.contains("openai.com") && api_key == "dummy" {
            return Err(anyhow!(
                "OPENAI_API_KEY environment variable not set for OpenAI URL"
            ));
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
            max_tokens,
            prompt_version,
            retries,
        })
    }

    pub async fn generate(&self, code: &str, context: &str) -> Result<String> {
        let prompt = if context.starts_with("File") {
            format!(
                "You are a code summarization assistant. Provide a concise summary (2-3 sentences) of this file.\n\
                Include:\n\
                1. The file's main responsibility\n\
                2. Key symbols/functions and their roles\n\
                3. How it fits into the broader system (if obvious)\n\
                Do NOT use bullet points. Write a cohesive paragraph.\n\
                Context: {}\n\
                Code:\n{}\n",
                context, code
            )
        } else {
            format!(
                "You are a code summarization assistant. Provide a concise, structured summary with the following fields:\n\
                - Intent: what the code does\n\
                - Inputs: parameters/inputs\n\
                - Outputs: return values/effects\n\
                - SideEffects: external IO/state changes\n\
                - Dependencies: important calls or types\n\
                Use short phrases, no bullets, max 4 sentences total.\n\
                Context: {}\n\
                Code:\n{}\n",
                context, code
            )
        };

        let mut last_err = None;
        for _ in 0..=self.retries {
            let res = self.client
                .post(&self.api_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&json!({
                    "model": self.model,
                    "messages": [
                        {"role": "system", "content": "You summarize code concisely with required fields."},
                        {"role": "user", "content": prompt}
                    ],
                    "max_tokens": self.max_tokens,
                    "temperature": 0.0
                }))
                .send()
                .await;

            match res {
                Ok(response) => match response.json::<serde_json::Value>().await {
                    Ok(res_val) => {
                        let choices = res_val.get("choices").and_then(|c| c.get(0));
                        if let Some(choice) = choices {
                            if let Some(content) = choice
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(|c| c.as_str())
                            {
                                let cleaned = content.trim().to_string();
                                if !cleaned.is_empty() {
                                    return Ok(cleaned);
                                }
                            }
                        }
                        last_err = Some(anyhow!("Invalid response from LLM: {:?}", res_val));
                    }
                    Err(e) => last_err = Some(anyhow!("Failed to parse JSON: {}", e)),
                },
                Err(e) => {
                    last_err = Some(anyhow!("LLM call failed: {}", e));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("Failed to generate summary")))
    }
}
