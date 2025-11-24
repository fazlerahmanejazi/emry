use crate::classifier::{ClassifiedQuery, QueryIntent};
use crate::llm::{Message, OpenAIProvider};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    #[serde(default)]
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub steps: Vec<PlanStep>,
}

pub struct Planner {
    llm: OpenAIProvider,
}

impl Planner {
    pub fn new(llm: OpenAIProvider) -> Self {
        Self { llm }
    }

    pub async fn plan(
        &self,
        question: &str,
        classified: &ClassifiedQuery,
        initial_summaries: Option<&str>,
        max_tokens: Option<u32>,
    ) -> Result<Plan> {
        let intent_hint = match classified.intent {
            QueryIntent::CodeIntrospection => "Plan: use search_symbols when names are hinted; use search_chunks on the topic; if a file is identified, call read_file_span for 50-100 lines around the relevant function.",
            QueryIntent::InfraFlow => "Plan: search_summaries and search_chunks_with_keywords for auth/login/token/payment/idempotent/subscription/db. Include graph_neighbors with ['calls','imports'] around any candidate node. If nothing is found, return final_answer_not_found with search coverage.",
            QueryIntent::OverviewReview => "Plan: start with search_summaries (repo/module/file) and list entry points via search_symbols (main/run/serve). Then search_chunks for core commands/services.",
            QueryIntent::Review => "Plan: 1) search_summaries (repo/module/file for architecture patterns), 2) list_entry_points (find main/CLI commands), 3) search_chunks_with_keywords (core abstractions: manager/store/index/graph/trait/error/config), 4) graph_neighbors from main entry points (understand component interactions).",
        };
        let keywords = if classified.domain_keywords.is_empty() {
            String::new()
        } else {
            format!("Focus keywords: {}", classified.domain_keywords.join(", "))
        };
        let secondary_hint = if classified.secondary_intents.is_empty() {
            String::new()
        } else {
            format!(
                "Secondary intents to also cover if possible: {}",
                classified
                    .secondary_intents
                    .iter()
                    .map(|i| format!("{:?}", i))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let summary_hint = initial_summaries.unwrap_or("");
        let prompt = format!(
            r#"You are a planning assistant for a codebase agent. Produce a JSON plan with 3-6 steps (never fewer than 3).
Each step must include action, description, params. Allowed actions: search_summaries, search_chunks, search_chunks_with_keywords, search_symbols, list_entry_points, graph_neighbors, graph_paths, read_file_span.
Return ONLY valid JSON matching this shape: {{"steps":[{{"action":"search_chunks","description":"...","params":{{}}}}]}}. No extra text.
{intent_hint}
{keywords}
{secondary_hint}
Context summaries:
{summary_hint}
Respond as JSON: {{"steps":[{{"id":"s1","action":"...","description":"...","params":{{}}}},...]}}
Question: {question}"#,
            intent_hint = intent_hint,
            keywords = keywords,
            secondary_hint = secondary_hint,
            summary_hint = summary_hint,
            question = question
        );
        let messages = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];
        let schema = json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "action": { "type": "string", "enum": [
                                "search_summaries",
                                "search_chunks",
                                "search_chunks_with_keywords",
                            "search_symbols",
                            "list_entry_points",
                            "graph_neighbors",
                            "graph_paths",
                            "read_file_span"
                            ]},
                            "description": { "type": "string" },
                            "params": { "type": "object" }
                        },
                        "required": ["action","description","params"],
                        "additionalProperties": false
                    },
                    "minItems": 1
                }
            },
            "required": ["steps"],
            "additionalProperties": false
        });
        let raw = match self
            .llm
            .chat_with_schema_and_limit(
                &messages,
                crate::llm::JsonSchemaSpec {
                    name: "agent_plan".to_string(),
                    schema,
                },
                max_tokens,
            )
            .await
        {
            Ok(s) => s,
            Err(_) => {
                return Ok(self.ensure_minimum_steps(
                    self.normalize_plan(self.fallback_plan(question, classified), classified),
                    question,
                    classified,
                ))
            }
        };
        let parsed =
            serde_json::from_str::<Plan>(&raw).or_else(|_| serde_json::from_str(raw.trim()));
        let plan = match parsed {
            Ok(plan) if !plan.steps.is_empty() => plan,
            _ => self.fallback_plan(question, classified),
        };
        let normalized = self.normalize_plan(plan, classified);
        let ensured = self.ensure_minimum_steps(normalized, question, classified);
        let ensured = self.append_final_answer(ensured);
        Ok(self.renumber_steps(ensured))
    }

    /// Ensure steps have ids/descriptions and inject keywords/graph hints where helpful.
    pub fn normalize_plan(&self, mut plan: Plan, classified: &ClassifiedQuery) -> Plan {
        for step in plan.steps.iter_mut() {
            if step.description.trim().is_empty() {
                step.description = step.action.clone();
            }
            // Auto-upgrade search_chunks to keyworded variant when keywords are present.
            if step.action == "search_chunks" && !classified.domain_keywords.is_empty() {
                step.action = "search_chunks_with_keywords".to_string();
                step.params["keywords"] = serde_json::Value::Array(
                    classified
                        .domain_keywords
                        .iter()
                        .map(|k| serde_json::Value::String(k.clone()))
                        .collect(),
                );
            }
            // Inject keywords into summary/symbol searches for InfraFlow.
            if matches!(classified.intent, QueryIntent::InfraFlow)
                && (step.action == "search_summaries" || step.action == "search_symbols")
                && !classified.domain_keywords.is_empty()
            {
                step.params["keywords"] = serde_json::Value::Array(
                    classified
                        .domain_keywords
                        .iter()
                        .map(|k| serde_json::Value::String(k.clone()))
                        .collect(),
                );
            }
        }
        // For InfraFlow, add a graph_paths step if missing and ensure graph_neighbors exists.
        if matches!(classified.intent, QueryIntent::InfraFlow) {
            // Seed default infra keywords if empty to cover common flows.
            if classified.domain_keywords.is_empty() {
                plan.steps.insert(
                    0,
                    PlanStep {
                        id: String::new(),
                        action: "search_chunks_with_keywords".to_string(),
                        description: "Search for infra terms".to_string(),
                        params: serde_json::json!({
                            "query": "auth login token payment subscription idempotent db",
                            "keywords": ["auth","login","token","payment","subscription","db"]
                        }),
                    },
                );
            }
            if !plan.steps.iter().any(|s| s.action == "graph_neighbors") {
                plan.steps.insert(
                    0,
                    PlanStep {
                        id: String::new(),
                        action: "graph_neighbors".to_string(),
                        description: "Trace neighbors for inferred infra nodes".to_string(),
                        params: serde_json::json!({ "node": classified.domain_keywords.get(0).cloned().unwrap_or_else(|| "main".to_string()), "kinds": ["calls","imports"], "max_hops": 3 }),
                    },
                );
            }
            if !plan.steps.iter().any(|s| s.action == "graph_paths") {
                let start = classified
                    .domain_keywords
                    .get(0)
                    .cloned()
                    .unwrap_or_else(|| "main".to_string());
                let target = classified
                    .domain_keywords
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| "db".to_string());
                plan.steps.push(PlanStep {
                    id: String::new(),
                    action: "graph_paths".to_string(),
                    description: "Trace paths between inferred infra nodes".to_string(),
                    params: serde_json::json!({ "start": start, "target": target, "max_hops": 4, "kinds": ["calls","imports"] }),
                });
            }
        }
        if matches!(
            classified.intent,
            QueryIntent::Review | QueryIntent::OverviewReview
        ) {
            if !plan.steps.iter().any(|s| s.action == "list_entry_points") {
                plan.steps.insert(
                    0,
                    PlanStep {
                        id: String::new(),
                        action: "list_entry_points".to_string(),
                        description: "List entry points".to_string(),
                        params: serde_json::json!({}),
                    },
                );
            }
            // For Review mode, add architecture-focused search
            if matches!(classified.intent, QueryIntent::Review) {
                if !plan.steps.iter().any(|s| {
                    s.action == "search_chunks_with_keywords"
                        && s.description.contains("architecture")
                }) {
                    plan.steps.insert(
                        1,
                        PlanStep {
                            id: String::new(),
                            action: "search_chunks_with_keywords".to_string(),
                            description: "Search for core abstractions and architecture patterns".to_string(),
                            params: serde_json::json!({
                                "query": "architecture components manager store index",
                            "keywords": ["manager", "store", "index", "graph", "trait", "error", "config"]
                            }),
                        },
                    );
                }
                // Add graph_neighbors to understand component interactions
                if !plan.steps.iter().any(|s| s.action == "graph_neighbors") {
                    plan.steps.push(PlanStep {
                        id: String::new(),
                        action: "graph_neighbors".to_string(),
                        description: "Trace component interactions from entry points".to_string(),
                        params: serde_json::json!({ "node": "main", "kinds": ["calls", "imports"], "max_hops": 2 }),
                    });
                }
            }
        }
        // Ensure final answer step exists.
        if !plan
            .steps
            .iter()
            .any(|s| s.action.starts_with("final_answer"))
        {
            plan.steps.push(PlanStep {
                id: String::new(),
                action: "final_answer".to_string(),
                description: "Compose answer".to_string(),
                params: serde_json::json!({}),
            });
        }
        plan
    }

    /// Always append a final_answer marker at the end.
    fn append_final_answer(&self, mut plan: Plan) -> Plan {
        if !plan
            .steps
            .iter()
            .any(|s| s.action.starts_with("final_answer"))
        {
            plan.steps.push(PlanStep {
                id: String::new(),
                action: "final_answer".to_string(),
                description: "Compose answer".to_string(),
                params: serde_json::json!({}),
            });
        }
        plan
    }

    /// Assign simple sequential ids (s1, s2, ...) in execution order.
    fn renumber_steps(&self, mut plan: Plan) -> Plan {
        for (idx, step) in plan.steps.iter_mut().enumerate() {
            step.id = format!("s{}", idx + 1);
        }
        plan
    }

    /// Ensure plan has at least 3 actionable steps and ends with final answer.
    fn ensure_minimum_steps(
        &self,
        mut plan: Plan,
        question: &str,
        classified: &ClassifiedQuery,
    ) -> Plan {
        // Guarantee a search_summaries step for overview/review flows.
        if matches!(
            classified.intent,
            QueryIntent::OverviewReview | QueryIntent::Review
        ) && !plan.steps.iter().any(|s| s.action == "search_summaries")
        {
            plan.steps.insert(
                0,
                PlanStep {
                    id: "s0".to_string(),
                    action: "search_summaries".to_string(),
                    description: "Seed context summaries".to_string(),
                    params: serde_json::json!({ "query": "overview architecture" }),
                },
            );
        }
        // Ensure at least one code search.
        if !plan
            .steps
            .iter()
            .any(|s| s.action.starts_with("search_chunks"))
        {
            plan.steps.push(PlanStep {
                id: format!("s{}", plan.steps.len() + 1),
                action: "search_chunks".to_string(),
                description: "Search code for query terms".to_string(),
                params: serde_json::json!({ "query": question }),
            });
        }
        // Ensure final answer exists.
        if !plan
            .steps
            .iter()
            .any(|s| s.action.starts_with("final_answer"))
        {
            plan.steps.push(PlanStep {
                id: format!("s{}", plan.steps.len() + 1),
                action: "final_answer".to_string(),
                description: "Compose answer".to_string(),
                params: serde_json::json!({}),
            });
        }
        // Enforce minimum length of 3 steps.
        while plan.steps.len() < 3 {
            plan.steps.insert(
                plan.steps.len().saturating_sub(1),
                PlanStep {
                    id: format!("s{}", plan.steps.len() + 1),
                    action: "search_chunks".to_string(),
                    description: "Additional search pass".to_string(),
                    params: serde_json::json!({ "query": question }),
                },
            );
        }
        plan
    }

    /// Deterministic fallback plans when the LLM parser fails or returns empty.
    fn fallback_plan(&self, question: &str, classified: &ClassifiedQuery) -> Plan {
        match classified.intent {
            QueryIntent::OverviewReview | QueryIntent::Review => Plan {
                steps: vec![
                    PlanStep {
                        id: "s1".to_string(),
                        action: "search_summaries".to_string(),
                        description: "Find high-level overviews".to_string(),
                        params: serde_json::json!({ "query": "overview architecture" }),
                    },
                    PlanStep {
                        id: "s2".to_string(),
                        action: "search_chunks".to_string(),
                        description: "Search code for the question terms".to_string(),
                        params: serde_json::json!({ "query": question }),
                    },
                    PlanStep {
                        id: "final".to_string(),
                        action: "final_answer".to_string(),
                        description: "Compose answer".to_string(),
                        params: serde_json::json!({}),
                    },
                ],
            },
            QueryIntent::InfraFlow => Plan {
                steps: vec![
                    PlanStep {
                        id: "s1".to_string(),
                        action: "search_summaries".to_string(),
                        description: "Scan summaries for infra terms".to_string(),
                        params: serde_json::json!({ "query": "auth login token payment subscription db" }),
                    },
                    PlanStep {
                        id: "s2".to_string(),
                        action: "search_chunks_with_keywords".to_string(),
                        description: "Search code for infra terms".to_string(),
                        params: serde_json::json!({ "query": question, "keywords": classified.domain_keywords }),
                    },
                    PlanStep {
                        id: "s3".to_string(),
                        action: "graph_neighbors".to_string(),
                        description: "Trace calls/imports around a candidate node".to_string(),
                        params: serde_json::json!({ "node": "main", "kinds": ["calls","imports"] }),
                    },
                    PlanStep {
                        id: "final".to_string(),
                        action: "final_answer".to_string(),
                        description: "Compose answer".to_string(),
                        params: serde_json::json!({}),
                    },
                ],
            },
            QueryIntent::CodeIntrospection => Plan {
                steps: vec![
                    PlanStep {
                        id: "s1".to_string(),
                        action: "search_chunks".to_string(),
                        description: "Search code for function/algorithm".to_string(),
                        params: serde_json::json!({ "query": question }),
                    },
                    PlanStep {
                        id: "final".to_string(),
                        action: "final_answer".to_string(),
                        description: "Compose answer".to_string(),
                        params: serde_json::json!({}),
                    },
                ],
            },
        }
    }
}
