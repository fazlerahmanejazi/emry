use anyhow::Result;
use crate::llm::{ModelProvider, Message};
use serde::Deserialize;
use emry_core::models::ExpandedQuery;

pub struct QueryRewriter<P: ModelProvider> {
    provider: P,
}

impl<P: ModelProvider> QueryRewriter<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    pub async fn rewrite(&self, query: &str) -> Result<ExpandedQuery> {
        let system_prompt = r#"You are an expert code search assistant. Your goal is to expand the user's search query to improve retrieval recall.
        
        Analyze the query and provide:
        1. A list of domain-specific keywords or synonyms that might appear in the code (e.g., "auth" -> ["login", "session", "jwt", "credential"]).
        2. A brief description of the user's intent (e.g., "finding implementation", "checking usage").
        
        Return the result as a JSON object with keys: "keywords" (array of strings) and "intent" (string).
        Do not output markdown formatting, just the raw JSON."#;

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: format!("Query: {}", query),
            },
        ];

        let response = self.provider.chat(&messages).await?;
        
        let clean_response = response.trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        #[derive(Deserialize)]
        struct LLMResponse {
            keywords: Vec<String>,
            intent: String,
        }

        let llm_res: LLMResponse = serde_json::from_str(clean_response)
            .or_else(|_| {
                Ok::<LLMResponse, serde_json::Error>(LLMResponse { keywords: vec![], intent: "unknown".to_string() })
            })?;

        Ok(ExpandedQuery {
            original: query.to_string(),
            keywords: llm_res.keywords,
            intent: llm_res.intent,
        })
    }
}
