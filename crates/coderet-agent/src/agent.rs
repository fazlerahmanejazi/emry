use crate::classifier::{ClassifiedQuery, QueryClassifier, QueryDifficulty, QueryIntent};
use crate::context::RepoContext;
use crate::executor::{ExecutionResult, Executor, Observation};
use crate::llm::OpenAIProvider;
use crate::planner::{Plan, Planner};
use crate::synthesizer::Synthesizer;
use anyhow::{anyhow, Result};
use coderet_config::AgentConfig;
use std::sync::Arc;
use std::time::Instant;

pub struct AskAgent {
    classifier: QueryClassifier,
    planner: Planner,
    executor: Executor,
    synthesizer: Synthesizer,
    pub config: AgentConfig,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct AgentMetrics {
    pub total_duration_ms: u128,
    pub classification_ms: u128,
    pub planning_ms: u128,
    pub execution_ms: u128,
    pub synthesis_ms: u128,
    pub llm_calls: usize,
    pub total_steps: usize,
    pub tools_used: std::collections::HashMap<String, usize>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentAnswer {
    pub answer: String,
    pub observations: Vec<Observation>,
    pub plan: Plan,
    pub actions_run: Vec<String>,
    pub coverage: String,
    pub intent: crate::classifier::QueryIntent,
    pub classified: ClassifiedQuery,
    pub coverage_notes: Vec<String>,
    pub coverage_summary: crate::executor::CoverageSummary,
    pub metrics: AgentMetrics,
}

impl AskAgent {
    pub fn new(ctx: Arc<RepoContext>) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("OPENAI_API_KEY not set for agent LLM"))?;
        let model = ctx.config.llm.model.clone();
        let api_base = ctx
            .config
            .llm
            .api_base
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let llm_cls = OpenAIProvider::with_base(model.clone(), api_key.clone(), api_base.clone());
        let llm_plan = OpenAIProvider::with_base(model.clone(), api_key.clone(), api_base.clone());
        let llm_syn = OpenAIProvider::with_base(model, api_key, api_base);
        Ok(Self {
            classifier: QueryClassifier::new(llm_cls),
            planner: Planner::new(llm_plan),
            executor: Executor::new(ctx.clone()),
            synthesizer: Synthesizer::new(llm_syn),
            config: ctx.config.agent.clone(),
        })
    }

    pub async fn answer_question(
        &self,
        question: &str,
        top_k: usize,
        progress_logs: bool,
    ) -> Result<AgentAnswer> {
        let start = Instant::now();
        let mut metrics = AgentMetrics::default();

        // Classification
        if progress_logs {
            println!("🔍 Classifying query...");
        }
        let cls_start = Instant::now();
        let classified = self
            .classifier
            .classify(question, Some(self.config.max_tokens))
            .await
            .unwrap_or(ClassifiedQuery {
                intent: QueryIntent::OverviewReview,
                secondary_intents: Vec::new(),
                difficulty: QueryDifficulty::Simple,
                domain_keywords: Vec::new(),
            });
        metrics.classification_ms = cls_start.elapsed().as_millis();
        metrics.llm_calls += 1;
        if progress_logs {
            println!(
                "   intent={:?} difficulty={:?} keywords={:?} secondary={:?}",
                classified.intent,
                classified.difficulty,
                classified.domain_keywords,
                classified.secondary_intents
            );
        }

        let mut exec_cfg = self.config.clone();
        if classified.difficulty == QueryDifficulty::Complex {
            exec_cfg.max_steps = exec_cfg.max_steps.saturating_add(3);
            exec_cfg.max_total_evidence_lines =
                exec_cfg.max_total_evidence_lines.saturating_add(200);
        }
        let search_top_k = if classified.difficulty == QueryDifficulty::Complex {
            top_k.saturating_add(5)
        } else {
            top_k
        };

        // Seed planner with repo/module summaries if available.
        let summary_seed =
            if let Ok(sums) = self.executor.summaries.repo_and_module_summaries(4).await {
                if sums.is_empty() {
                    None
                } else {
                    Some(
                        sums.into_iter()
                            .map(|s| s.summary.text)
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                    )
                }
            } else {
                None
            };

        // Planning
        if progress_logs {
            println!("📋 Planning execution...");
        }
        let plan_start = Instant::now();
        let plan = self
            .planner
            .plan(
                question,
                &classified,
                summary_seed.as_deref(),
                Some(exec_cfg.max_tokens),
            )
            .await?;
        metrics.planning_ms = plan_start.elapsed().as_millis();
        metrics.llm_calls += 1;
        metrics.total_steps = plan.steps.len();
        if progress_logs {
            println!("   plan steps: {}", plan.steps.len());
        }

        // Execution
        if progress_logs {
            println!("⚙️  Executing {} step(s)...", plan.steps.len());
        }
        let exec_start = Instant::now();
        let ExecutionResult {
            observations,
            actions_run,
            coverage,
            total_evidence_lines,
            coverage_notes,
            coverage_summary,
        } = self
            .executor
            .execute(plan.clone(), search_top_k, &exec_cfg, progress_logs)
            .await;
        metrics.execution_ms = exec_start.elapsed().as_millis();

        // Track tool usage
        for action in &actions_run {
            *metrics.tools_used.entry(action.clone()).or_insert(0) += 1;
        }

        // Synthesis
        if progress_logs {
            println!("✍️  Synthesizing answer...");
        }
        let syn_start = Instant::now();
        let answer = self
            .synthesizer
            .synthesize(
                question,
                &observations,
                &plan,
                &actions_run,
                &exec_cfg,
                &classified,
                total_evidence_lines,
                &coverage_notes,
                &coverage_summary,
            )
            .await?;
        metrics.synthesis_ms = syn_start.elapsed().as_millis();
        metrics.llm_calls += 1;

        metrics.total_duration_ms = start.elapsed().as_millis();

        Ok(AgentAnswer {
            answer,
            observations,
            plan,
            actions_run,
            coverage,
            intent: classified.intent,
            classified,
            coverage_notes,
            coverage_summary,
            metrics,
        })
    }
}
