use crate::llm::{Message, OpenAIProvider};
use anyhow::{anyhow, Result};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum QueryIntent {
    CodeIntrospection,
    OverviewReview,
    InfraFlow,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum QueryDifficulty {
    Simple,
    Complex,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClassifiedQuery {
    pub intent: QueryIntent,
    pub secondary_intents: Vec<QueryIntent>,
    pub difficulty: QueryDifficulty,
    pub domain_keywords: Vec<String>,
}

pub struct QueryClassifier {
    llm: OpenAIProvider,
}

impl QueryClassifier {
    pub fn new(llm: OpenAIProvider) -> Self {
        Self { llm }
    }

    pub async fn classify(&self, query: &str, max_tokens: Option<u32>) -> Result<ClassifiedQuery> {
        let prompt = format!(
            concat!(
                "Classify the intent of this repository question into one of: ",
                "CodeIntrospection (asking about specific functions/algorithms), ",
                "OverviewReview (high-level purpose/architecture), ",
                "InfraFlow (auth/payments/subscriptions/DB flows), ",
                "Review (critique/review/architecture/quality).\n",
                "Also classify difficulty: Simple or Complex.\n",
                "Extract up to 6 domain keywords (lowercase, no spaces) that would help searching (e.g., auth, login, token, payments, chunking, summarizer).\n",
                "Respond as JSON: {{\"intent\":\"...\",\"difficulty\":\"...\",\"keywords\":[\"...\"]}}\n",
                "Query: {}"
            ),
            query
        );
        let messages = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];
        let schema = json!({
            "type": "object",
            "properties": {
                "intent": { "type": "string", "enum": ["CodeIntrospection","OverviewReview","InfraFlow","Review"] },
                "secondary_intents": { "type": "array", "items": { "type": "string", "enum": ["CodeIntrospection","OverviewReview","InfraFlow","Review"] } },
                "difficulty": { "type": "string", "enum": ["Simple","Complex"] },
                "keywords": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["intent","difficulty","keywords"],
            "additionalProperties": false
        });
        let resp = self
            .llm
            .chat_with_schema_and_limit(
                &messages,
                crate::llm::JsonSchemaSpec {
                    name: "query_classification".to_string(),
                    schema,
                },
                max_tokens,
            )
            .await?;
        let parsed: serde_json::Value = serde_json::from_str(&resp)
            .or_else(|_| serde_json::from_str(resp.trim()))
            .map_err(|e| anyhow!("failed to parse classifier response: {}", e))?;
        let intent = match parsed["intent"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "codeintrospection" => QueryIntent::CodeIntrospection,
            "infraflow" => QueryIntent::InfraFlow,
            "review" => QueryIntent::Review,
            _ => QueryIntent::OverviewReview,
        };
        let difficulty = match parsed["difficulty"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "complex" => QueryDifficulty::Complex,
            _ => QueryDifficulty::Simple,
        };
        let secondary_intents = parsed["secondary_intents"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| match s.to_lowercase().as_str() {
                        "codeintrospection" => Some(QueryIntent::CodeIntrospection),
                        "infraflow" => Some(QueryIntent::InfraFlow),
                        "review" => Some(QueryIntent::Review),
                        "overviewreview" => Some(QueryIntent::OverviewReview),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let keywords = parsed["keywords"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_lowercase()))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(ClassifiedQuery {
            intent,
            secondary_intents,
            difficulty,
            domain_keywords: keywords,
        })
    }
}
