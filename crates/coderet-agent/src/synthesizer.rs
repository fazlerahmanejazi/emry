use crate::classifier::{ClassifiedQuery, QueryIntent};
use crate::executor::Observation;
use crate::llm::{Message, OpenAIProvider};
use crate::planner::Plan;
use anyhow::Result;



pub struct Synthesizer {
    llm: OpenAIProvider,
}

impl Synthesizer {
    pub fn new(llm: OpenAIProvider) -> Self {
        Self { llm }
    }

    pub async fn synthesize(
        &self,
        question: &str,
        observations: &[Observation],
        plan: &Plan,
        actions_run: &[String],
        cfg: &coderet_config::AgentConfig,
        classified: &ClassifiedQuery,
        total_evidence_lines: usize,
        coverage_notes: &[String],
        coverage_summary: &crate::executor::CoverageSummary,
    ) -> Result<String> {
        let no_hits = coverage_summary.search_hits == 0
            && coverage_summary.summary_hits == 0
            && coverage_summary.symbol_hits == 0
            && coverage_summary.graph_hits == 0
            && coverage_summary.file_reads == 0;

        if observations.is_empty() || total_evidence_lines == 0 || no_hits {
            let actions = if actions_run.is_empty() {
                "no actions executed".to_string()
            } else {
                format!("actions executed: {}", actions_run.join(", "))
            };
            let coverage = if cfg.coverage_on_empty {
                format!(
                    concat!(
                        "\nCoverage summary:\n",
                        "  plan steps: {}.\n",
                        "  Actions run: {}.\n",
                        "  Keywords: {}.\n",
                        "  Notes: {}.\n",
                        "  Queries: {:?} {:?} {:?}.\n",
                        "  GraphNodes: {:?}.\n",
                        "  Dirs: {:?}"
                    ),
                    plan.steps.len(),
                    actions_run.join(", "),
                    classified.domain_keywords.join(", "),
                    if coverage_notes.is_empty() {
                        "none".to_string()
                    } else {
                        coverage_notes.join(" | ")
                    },
                    coverage_summary.search_queries,
                    coverage_summary.summary_queries,
                    coverage_summary.symbol_queries,
                    coverage_summary.graph_nodes,
                    coverage_summary.scanned_dirs,
                )
            } else {
                String::new()
            };
            let absent = match classified.intent {
                QueryIntent::InfraFlow => "No relevant code for this flow found.",
                _ => "No relevant evidence found.",
            };
            return Ok(format!(
                "{} Unable to answer '{}'. Searched via {}.{}",
                absent, question, actions, coverage
            ));
        }
        let mut obs_text = String::new();
        for (i, obs) in observations.iter().enumerate() {
            obs_text.push_str(&format!(
                "OBS {} ({} - {}):\n",
                i + 1,
                obs.step_id,
                obs.action
            ));
            if let Some(err) = &obs.error {
                obs_text.push_str(&format!("  error: {}\n\n", err));
                continue;
            }
            for ev in &obs.evidence {
                let loc = ev
                    .file_path
                    .as_ref()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "n/a".to_string());
                let range = match (ev.start_line, ev.end_line) {
                    (Some(s), Some(e)) => format!("{}-{}", s, e),
                    _ => "n/a".to_string(),
                };
                let tags = if ev.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", ev.tags.join(", "))
                };
                let fence_lang = sniff_lang_from_path(&loc);
                obs_text.push_str(&format!(
                    "  [{}:{}]{} {}\n```{}\n{}\n```\n\n",
                    loc, range, tags, ev.source, fence_lang, ev.text
                ));
            }
        }
        let mut plan_text = String::new();
        for (i, step) in plan.steps.iter().enumerate() {
            plan_text.push_str(&format!(
                "STEP {}: {} {}\n",
                i + 1,
                step.action,
                step.params
            ));
        }
        let instructions = match classified.intent {
            QueryIntent::Review => crate::prompts::REVIEW_PROMPT,
            QueryIntent::InfraFlow => crate::prompts::INFRA_FLOW_PROMPT,
            QueryIntent::CodeIntrospection => crate::prompts::CODE_INTROSPECTION_PROMPT,
            QueryIntent::OverviewReview => crate::prompts::OVERVIEW_REVIEW_PROMPT,
        };
        let prompt = format!(
            concat!(
                "{}\n\n",
                "Question: {}\n\n",
                "Plan:\n{}\n",
                "Observations:\n{}\n",
                "Coverage: {}\n",
                "Keywords: {}\n",
                "HitSummary: search={}, summaries={}, symbols={}, graph={}, files={}\n",
                "SearchQueries: {:?}\n",
                "SummaryQueries: {:?}\n",
                "SymbolQueries: {:?}\n",
                "GraphNodes: {:?}\n",
                "DirsScanned: {:?}\n"
            ),
            instructions,
            question,
            plan_text,
            obs_text,
            if coverage_notes.is_empty() {
                "n/a".to_string()
            } else {
                coverage_notes.join(" | ")
            },
            classified.domain_keywords.join(", "),
            coverage_summary.search_hits,
            coverage_summary.summary_hits,
            coverage_summary.symbol_hits,
            coverage_summary.graph_hits,
            coverage_summary.file_reads,
            coverage_summary.search_queries,
            coverage_summary.summary_queries,
            coverage_summary.symbol_queries,
            coverage_summary.graph_nodes,
            coverage_summary.scanned_dirs,
        );
        let messages = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];
        let resp = self
            .llm
            .chat_with_limit(&messages, Some(cfg.max_tokens))
            .await?;
        Ok(resp)
    }
}

fn sniff_lang_from_path(path: &str) -> &str {
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        match ext {
            "rs" => "rust",
            "ts" | "tsx" => "ts",
            "js" | "jsx" => "js",
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "rb" => "ruby",
            "cs" => "csharp",
            "cpp" | "cc" | "cxx" => "cpp",
            "c" => "c",
            "kt" => "kotlin",
            "swift" => "swift",
            "php" => "php",
            _ => "",
        }
    } else {
        ""
    }
}
