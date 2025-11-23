use super::Embedder;
use anyhow::{anyhow, Result};
use reqwest::Client;

pub struct OllamaEmbedder {
    client: Client,
    model: String,
    base_url: String,
}

impl OllamaEmbedder {
    pub fn new(model: Option<String>) -> Result<Self> {
        let model_name = model.unwrap_or_else(|| "nomic-embed-text".to_string());
        let base = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
        Ok(Self {
            client: Client::builder().build()?,
            model: model_name,
            base_url: base,
        })
    }
}

impl Embedder for OllamaEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Use tokio's current runtime handle
        let runtime = tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| {
                tokio::runtime::Runtime::new()
                    .ok()
                    .map(|rt| rt.handle().clone())
            })
            .ok_or_else(|| anyhow!("No tokio runtime available"))?;

        runtime.block_on(async {
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                let resp = self
                    .client
                    .post(format!("{}/api/embeddings", self.base_url))
                    .json(&serde_json::json!({
                        "model": self.model,
                        "prompt": t
                    }))
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?;

                if let Some(arr) = resp.get("embedding").and_then(|v| v.as_array()) {
                    let mut vec = Vec::with_capacity(arr.len());
                    for v in arr {
                        vec.push(v.as_f64().unwrap_or(0.0) as f32);
                    }
                    out.push(vec);
                } else {
                    return Err(anyhow!("Invalid response from Ollama: {:?}", resp));
                }
            }
            Ok(out)
        })
    }
}
