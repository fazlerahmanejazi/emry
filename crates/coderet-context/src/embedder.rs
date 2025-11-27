use anyhow::{anyhow, Result};
use async_trait::async_trait;
use coderet_config::{EmbeddingBackend, EmbeddingConfig};
use coderet_core::traits::Embedder;
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::sync::Arc;

/// Select an embedder based on configuration and available environment variables.
/// Mirrors the CLI selection logic: prefer OpenAI if key is set, otherwise try Ollama.
pub async fn select_embedder(config: &EmbeddingConfig) -> Result<Arc<dyn Embedder + Send + Sync>> {
    if let Ok(api_key) = env::var("OPENAI_API_KEY") {
        let model = if config.backend == EmbeddingBackend::External && !config.model_name.is_empty()
        {
            config.model_name.clone()
        } else {
            "text-embedding-3-small".to_string()
        };
        match ExternalEmbedder::new(model, api_key) {
            Ok(ext) => return Ok(Arc::new(ext)),
            Err(err) => eprintln!("Failed to init ExternalEmbedder: {}", err),
        }
    }

    // Ollama fallback
    let ollama_model = if config.model_name.is_empty() {
        "nomic-embed-text".to_string()
    } else {
        config.model_name.clone()
    };
    match OllamaEmbedder::new(ollama_model) {
        Ok(ollama) => Ok(Arc::new(ollama)),
        Err(err) => {
            eprintln!("Failed to init Ollama embedder: {}", err);
            Err(anyhow!("No embedder available"))
        }
    }
}

struct ExternalEmbedder {
    model: String,
    api_key: String,
    client: Client,
}

impl ExternalEmbedder {
    fn new(model: String, api_key: String) -> Result<Self> {
        Ok(Self {
            model,
            api_key,
            client: Client::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingItem {
    embedding: Vec<f32>,
}

#[async_trait]
impl Embedder for ExternalEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut list = self.embed_batch(&[text.to_string()]).await?;
        list.pop()
            .ok_or_else(|| anyhow!("Empty embedding response"))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let resp = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "input": texts,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI embeddings failed ({}): {}", status, body));
        }

        let parsed: OpenAIEmbeddingResponse = resp.json().await?;
        if parsed.data.len() != texts.len() {
            return Err(anyhow!(
                "Mismatch embedding count: got {}, expected {}",
                parsed.data.len(),
                texts.len()
            ));
        }
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}

struct OllamaEmbedder {
    model: String,
    base_url: String,
    client: Client,
}

impl OllamaEmbedder {
    fn new(model: String) -> Result<Self> {
        let base_url =
            env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
        Ok(Self {
            model,
            base_url,
            client: Client::new(),
        })
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let resp = self
            .client
            .post(format!(
                "{}/api/embeddings",
                self.base_url.trim_end_matches('/')
            ))
            .json(&serde_json::json!({
                "model": self.model,
                "prompt": text,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama embeddings failed ({}): {}", status, body));
        }

        let json: serde_json::Value = resp.json().await?;
        let embedding = json["embedding"]
            .as_array()
            .ok_or_else(|| anyhow!("No embedding field in Ollama response"))?
            .iter()
            .filter_map(|v| v.as_f64())
            .map(|f| f as f32)
            .collect::<Vec<f32>>();
        Ok(embedding)
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed(text).await?);
        }
        Ok(embeddings)
    }
}
