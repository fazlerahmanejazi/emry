use super::Embedder;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::env;

pub struct ExternalEmbedder {
    client: reqwest::Client,
    api_key: String,
    model: String,
    api_url: String,
}

impl ExternalEmbedder {
    pub fn new(model: Option<String>) -> Result<Self> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("OPENAI_API_KEY environment variable not set"))?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            api_url: "https://api.openai.com/v1/embeddings".to_string(),
        })
    }
}

impl Embedder for ExternalEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // OpenAI API allows batching
        let res = tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                self.client
                    .post(&self.api_url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&json!({
                        "input": texts,
                        "model": self.model
                    }))
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await
            })
        })
        .map_err(|e| anyhow!("Failed to call OpenAI API: {}", e))?;

        // Parse response
        let data = res
            .get("data")
            .ok_or_else(|| anyhow!("Invalid response from OpenAI: {:?}", res))?;
        let data_array = data
            .as_array()
            .ok_or_else(|| anyhow!("Expected data array"))?;

        let mut embeddings = Vec::new();
        for item in data_array {
            let emb_val = item
                .get("embedding")
                .ok_or_else(|| anyhow!("Missing embedding field"))?;
            let emb_vec: Vec<f32> = serde_json::from_value(emb_val.clone())?;
            embeddings.push(emb_vec);
        }

        Ok(embeddings)
    }
}
