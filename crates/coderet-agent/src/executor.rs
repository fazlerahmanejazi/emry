use crate::context::RepoContext;
use crate::planner::Plan;
use crate::tools::{fs::FsTool, graph::GraphTool, search::SearchTool, summaries::SummaryTool};
use crate::types::GraphSubgraph;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EvidenceChunk {
    pub source: String,
    pub file_path: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub text: String,
    pub score: Option<f32>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Observation {
    pub step_id: String,
    pub action: String,
    pub description: String,
    pub evidence: Vec<EvidenceChunk>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionResult {
    pub observations: Vec<Observation>,
    pub actions_run: Vec<String>,
    pub coverage: String,
    pub total_evidence_lines: usize,
    pub coverage_notes: Vec<String>,
    pub coverage_summary: CoverageSummary,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CoverageSummary {
    pub search_hits: usize,
    pub summary_hits: usize,
    pub symbol_hits: usize,
    pub graph_hits: usize,
    pub file_reads: usize,
    pub search_queries: Vec<String>,
    pub summary_queries: Vec<String>,
    pub symbol_queries: Vec<String>,
    pub graph_nodes: Vec<String>,
    pub scanned_dirs: Vec<String>,
}

impl CoverageSummary {
    pub fn total_hits(&self) -> usize {
        self.search_hits + self.summary_hits + self.symbol_hits + self.graph_hits + self.file_reads
    }
}

pub struct Executor {
    pub summaries: SummaryTool,
    pub search: SearchTool,
    graph: GraphTool,
    fs: FsTool,
}

impl Executor {
    pub fn new(ctx: Arc<RepoContext>) -> Self {
        Self {
            search: SearchTool::new(ctx.clone()),
            summaries: SummaryTool::new(ctx.clone()),
            graph: GraphTool::new(ctx.clone()),
            fs: FsTool::new(ctx),
        }
    }

    pub async fn execute(
        &self,
        plan: Plan,
        top_k: usize,
        cfg: &coderet_config::AgentConfig,
        verbose: bool,
    ) -> ExecutionResult {
        let mut obs: Vec<Observation> = Vec::new();
        let mut actions = Vec::new();
        let max_per_step = cfg.max_per_step;
        let max_total = cfg.max_observations;
        let mut steps_run = 0usize;
        let mut total_lines: usize = 0;
        let mut coverage_notes: Vec<String> = Vec::new();
        let mut coverage_summary = CoverageSummary::default();

        let steps_total = plan.steps.len();
        for (idx, step) in plan.steps.into_iter().enumerate() {
            if steps_run >= cfg.max_steps || obs.len() >= max_total {
                break;
            }
            if verbose {
                println!(
                    "  → step {}/{}: {} {}",
                    idx + 1,
                    steps_total,
                    step.action,
                    step.params
                );
            }
            let timed = timeout(Duration::from_secs(cfg.step_timeout_secs), async {
                match step.action.as_str() {
                    "search_chunks" => {
                        actions.push("search_chunks".to_string());
                        if let Some(q) = step.params["query"].as_str() {
                            coverage_notes.push(format!("search_chunks q='{}'", q));
                            coverage_summary.search_queries.push(q.to_string());
                        }
                        let mut ev = Vec::new();
                        if let Ok(results) = self
                            .search
                            .search_chunks(step.params["query"].as_str().unwrap_or_default(), top_k)
                            .await
                        {
                            coverage_summary.search_hits += results.len();
                            if verbose {
                                let query = step.params["query"].as_str().unwrap_or("");
                                println!(
                                    "  → search_chunks: \"{}\" ({} results)",
                                    query,
                                    results.len()
                                );
                            }
                            for hit in results.into_iter().take(max_per_step) {
                                let chunk = hit.chunk;
                                let lines: Vec<&str> = chunk.content.lines().collect();
                                let allowed = cfg
                                    .max_total_evidence_lines
                                    .saturating_sub(total_lines)
                                    .max(0);
                                if allowed == 0 {
                                    break;
                                }
                                let snippet = lines
                                    .iter()
                                    .take(allowed.min(lines.len()))
                                    .map(|s| *s)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                total_lines += snippet.lines().count();
                                ev.push(EvidenceChunk {
                                    source: "search_chunks".to_string(),
                                    file_path: Some(chunk.file_path.to_string_lossy().to_string()),
                                    start_line: Some(chunk.start_line as usize),
                                    end_line: Some(chunk.end_line as usize),
                                    text: snippet,
                                    score: Some(hit.score),
                                    tags: Vec::new(),
                                });
                                if total_lines >= cfg.max_total_evidence_lines {
                                    break;
                                }
                                if let Some(parent) = chunk.file_path.parent() {
                                    let dir = parent.to_string_lossy().to_string();
                                    if !coverage_summary.scanned_dirs.contains(&dir) {
                                        coverage_summary.scanned_dirs.push(dir);
                                    }
                                }
                            }
                        } else {
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: Vec::new(),
                                error: Some("search_chunks failed".to_string()),
                            });
                            return;
                        }
                        obs.push(Observation {
                            step_id: step.id.clone(),
                            action: step.action.clone(),
                            description: step.description.clone(),
                            evidence: ev,
                            error: None,
                        });
                    }
                    "search_chunks_with_keywords" => {
                        actions.push("search_chunks_with_keywords".to_string());
                        let mut ev = Vec::new();
                        let keywords: Vec<String> = step
                            .params
                            .get("keywords")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if let Some(q) = step.params["query"].as_str() {
                            coverage_notes.push(format!(
                                "search_chunks_with_keywords q='{}' keywords={:?}",
                                q, keywords
                            ));
                            coverage_summary.search_queries.push(q.to_string());
                        }
                        if let Ok(results) = self
                            .search
                            .search_chunks_with_keywords(
                                step.params["query"].as_str().unwrap_or_default(),
                                &keywords,
                                top_k,
                            )
                            .await
                        {
                            coverage_summary.search_hits += results.len();
                            for hit in results.into_iter().take(max_per_step) {
                                let chunk = hit.chunk;
                                let lines: Vec<&str> = chunk.content.lines().collect();
                                let allowed = cfg
                                    .max_total_evidence_lines
                                    .saturating_sub(total_lines)
                                    .max(0);
                                if allowed == 0 {
                                    break;
                                }
                                let snippet = lines
                                    .iter()
                                    .take(allowed.min(lines.len()))
                                    .map(|s| *s)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                total_lines += snippet.lines().count();
                                ev.push(EvidenceChunk {
                                    source: "search_chunks_with_keywords".to_string(),
                                    file_path: Some(chunk.file_path.to_string_lossy().to_string()),
                                    start_line: Some(chunk.start_line as usize),
                                    end_line: Some(chunk.end_line as usize),
                                    text: snippet,
                                    score: Some(hit.score),
                                    tags: keywords.clone(),
                                });
                                if total_lines >= cfg.max_total_evidence_lines {
                                    break;
                                }
                                if let Some(parent) = chunk.file_path.parent() {
                                    let dir = parent.to_string_lossy().to_string();
                                    if !coverage_summary.scanned_dirs.contains(&dir) {
                                        coverage_summary.scanned_dirs.push(dir);
                                    }
                                }
                            }
                            coverage_notes
                                .push(format!("search_chunks_with_keywords hits={}", ev.len()));
                        } else {
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: Vec::new(),
                                error: Some("search_chunks_with_keywords failed".to_string()),
                            });
                            return;
                        }
                        obs.push(Observation {
                            step_id: step.id.clone(),
                            action: step.action.clone(),
                            description: step.description.clone(),
                            evidence: ev,
                            error: None,
                        });
                    }
                    "search_summaries" => {
                        actions.push("search_summaries".to_string());
                        if let Some(q) = step.params["query"].as_str() {
                            coverage_notes.push(format!("search_summaries q='{}'", q));
                            coverage_summary.summary_queries.push(q.to_string());
                        }
                        let mut ev = Vec::new();
                        if let Ok(results) = self
                            .summaries
                            .search_summaries(
                                step.params["query"].as_str().unwrap_or_default(),
                                top_k,
                            )
                            .await
                        {
                            coverage_summary.summary_hits += results.len();
                            if verbose {
                                let query = step.params["query"].as_str().unwrap_or("");
                                println!(
                                    "  → search_summaries: \"{}\" ({} summaries)",
                                    query,
                                    results.len()
                                );
                            }
                            for hit in results.into_iter().take(max_per_step) {
                                let loc = hit
                                    .summary
                                    .file_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_else(|| hit.summary.target_id.clone());
                                let allowed = cfg
                                    .max_total_evidence_lines
                                    .saturating_sub(total_lines)
                                    .max(0);
                                if allowed == 0 {
                                    break;
                                }
                                let snippet = hit
                                    .summary
                                    .text
                                    .lines()
                                    .take(allowed)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                total_lines += snippet.lines().count();
                                ev.push(EvidenceChunk {
                                    source: "search_summaries".to_string(),
                                    file_path: Some(loc),
                                    start_line: hit.summary.start_line,
                                    end_line: hit.summary.end_line,
                                    text: snippet,
                                    score: Some(hit.score),
                                    tags: vec![format!("{:?}", hit.summary.kind)],
                                });
                                if total_lines >= cfg.max_total_evidence_lines {
                                    break;
                                }
                                if let Some(fp) = &hit.summary.file_path {
                                    if let Some(parent) = fp.parent() {
                                        let dir = parent.to_string_lossy().to_string();
                                        if !coverage_summary.scanned_dirs.contains(&dir) {
                                            coverage_summary.scanned_dirs.push(dir);
                                        }
                                    }
                                }
                            }
                        } else {
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: Vec::new(),
                                error: Some("search_summaries failed".to_string()),
                            });
                            return;
                        }
                        obs.push(Observation {
                            step_id: step.id.clone(),
                            action: step.action.clone(),
                            description: step.description.clone(),
                            evidence: ev,
                            error: None,
                        });
                    }
                    "search_symbols" => {
                        actions.push("search_symbols".to_string());
                        if let Some(q) = step.params["query"].as_str() {
                            coverage_notes.push(format!("search_symbols q='{}'", q));
                            coverage_summary.symbol_queries.push(q.to_string());
                        }
                        let mut ev = Vec::new();
                        if let Ok(results) = self
                            .search
                            .search_symbols(step.params["query"].as_str().unwrap_or_default())
                        {
                            coverage_summary.symbol_hits += results.len();
                            if verbose {
                                let query = step.params["query"].as_str().unwrap_or("");
                                println!(
                                    "  → search_symbols: \"{}\" ({} symbols)",
                                    query,
                                    results.len()
                                );
                            }
                            for sym in results.into_iter().take(max_per_step) {
                                ev.push(EvidenceChunk {
                                    source: "search_symbols".to_string(),
                                    file_path: Some(sym.file_path.clone()),
                                    start_line: Some(sym.start_line),
                                    end_line: Some(sym.end_line),
                                    text: format!("Symbol {} ({})", sym.name, sym.symbol.kind),
                                    score: None,
                                    tags: vec![sym.symbol.kind.clone()],
                                });
                                if ev.len() >= max_per_step {
                                    break;
                                }
                            }
                        }
                        obs.push(Observation {
                            step_id: step.id.clone(),
                            action: step.action.clone(),
                            description: step.description.clone(),
                            evidence: ev,
                            error: None,
                        });
                    }
                    "graph_neighbors" => {
                        actions.push("graph_neighbors".to_string());
                        let node_id = step.params["node"].as_str().unwrap_or_default();
                        let kinds: Vec<String> = step
                            .params
                            .get("kinds")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !node_id.is_empty() {
                            coverage_summary.graph_nodes.push(node_id.to_string());
                        }
                        if let Ok(sub) = self.graph.neighbors(node_id, &kinds, 3) {
                            coverage_summary.graph_hits += sub.edges.len();
                            if verbose {
                                println!(
                                    "    graph_neighbors node='{}' kinds={:?} edges={}",
                                    node_id,
                                    kinds,
                                    sub.edges.len()
                                );
                            }
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: vec![EvidenceChunk {
                                    source: "graph_neighbors".to_string(),
                                    file_path: None,
                                    start_line: None,
                                    end_line: None,
                                    text: render_graph(&sub),
                                    score: None,
                                    tags: kinds.clone(),
                                }],
                                error: None,
                            });
                        } else {
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: Vec::new(),
                                error: Some("graph_neighbors failed".to_string()),
                            });
                        }
                    }
                    "graph_paths" => {
                        actions.push("graph_paths".to_string());
                        let start = step.params["start"].as_str().unwrap_or_default();
                        let target = step.params["target"].as_str().unwrap_or_default();
                        let max_hops = step.params["max_hops"].as_u64().unwrap_or(4) as usize;
                        let kinds: Vec<String> = step
                            .params
                            .get("kinds")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_else(|| vec!["calls".to_string(), "imports".to_string()]);
                        coverage_notes.push(format!(
                            "graph_paths from '{}' to '{}' hops={} kinds={:?}",
                            start, target, max_hops, kinds
                        ));
                        if verbose {
                            println!(
                                "    graph_paths start='{}' target='{}' hops={} kinds={:?}",
                                start, target, max_hops, kinds
                            );
                        }
                        if let Ok(paths) = self
                            .graph
                            .shortest_paths_with_kinds(start, target, &kinds, max_hops)
                        {
                            let mut ev = Vec::new();
                            for p in paths.into_iter().take(max_per_step) {
                                coverage_summary.graph_hits += 1;
                                ev.push(EvidenceChunk {
                                    source: "graph_paths".to_string(),
                                    file_path: None,
                                    start_line: None,
                                    end_line: None,
                                    text: p.join(" -> "),
                                    score: None,
                                    tags: vec![format!("{}->{}", start, target)],
                                });
                            }
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: ev,
                                error: None,
                            });
                        } else {
                            obs.push(Observation {
                                step_id: step.id.clone(),
                                action: step.action.clone(),
                                description: step.description.clone(),
                                evidence: Vec::new(),
                                error: Some("graph_paths failed".to_string()),
                            });
                        }
                    }
                    "read_file_span" => {
                        actions.push("read_file_span".to_string());
                        if let Some(path) = step.params["path"].as_str() {
                            let start = step.params["start"].as_u64().unwrap_or(1) as usize;
                            let end = step.params["end"]
                                .as_u64()
                                .unwrap_or_else(|| (start + 80) as u64)
                                as usize;
                            match self
                                .fs
                                .read_file_span(std::path::Path::new(path), start, end)
                            {
                                Ok(text) => {
                                    coverage_summary.file_reads += 1;
                                    if verbose {
                                        println!(
                                            "  → read_file_span: {}:{}-{} ✓",
                                            path, start, end
                                        );
                                    }
                                    let allowed = cfg
                                        .max_total_evidence_lines
                                        .saturating_sub(total_lines)
                                        .max(0);
                                    let snippet = text
                                        .lines()
                                        .take(allowed.max(1))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    total_lines += snippet.lines().count();
                                    obs.push(Observation {
                                        step_id: step.id.clone(),
                                        action: step.action.clone(),
                                        description: step.description.clone(),
                                        evidence: vec![EvidenceChunk {
                                            source: "read_file_span".to_string(),
                                            file_path: Some(path.to_string()),
                                            start_line: Some(start),
                                            end_line: Some(end),
                                            text: snippet,
                                            score: None,
                                            tags: Vec::new(),
                                        }],
                                        error: None,
                                    });
                                }
                                Err(e) => obs.push(Observation {
                                    step_id: step.id.clone(),
                                    action: step.action.clone(),
                                    description: step.description.clone(),
                                    evidence: Vec::new(),
                                    error: Some(format!("read_file_span failed: {}", e)),
                                }),
                            }
                        }
                    }
                    "list_entry_points" => {
                        actions.push("list_entry_points".to_string());
                        match self.search.list_entry_points() {
                            Ok(entries) => {
                                coverage_summary.symbol_hits += entries.len();
                                let mut ev = Vec::new();
                                for ep in entries.into_iter().take(max_per_step) {
                                    ev.push(EvidenceChunk {
                                        source: "list_entry_points".to_string(),
                                        file_path: Some(ep.file_path.clone()),
                                        start_line: Some(ep.start_line),
                                        end_line: Some(ep.end_line),
                                        text: format!("Entry point {}", ep.name),
                                        score: None,
                                        tags: vec![ep.symbol.kind.clone()],
                                    });
                                }
                                obs.push(Observation {
                                    step_id: step.id.clone(),
                                    action: step.action.clone(),
                                    description: step.description.clone(),
                                    evidence: ev,
                                    error: None,
                                });
                            }
                            Err(e) => {
                                obs.push(Observation {
                                    step_id: step.id.clone(),
                                    action: step.action.clone(),
                                    description: step.description.clone(),
                                    evidence: Vec::new(),
                                    error: Some(format!("list_entry_points failed: {}", e)),
                                });
                                return;
                            }
                        }
                    }
                    "final_answer" => {
                        actions.push("final_answer".to_string());
                        // handled by synthesizer
                    }
                    "final_answer_not_found" => {
                        actions.push("final_answer_not_found".to_string());
                        coverage_notes.push("final_answer_not_found invoked".to_string());
                    }
                    _ => {
                        actions.push(step.action.clone());
                        obs.push(Observation {
                            step_id: step.id.clone(),
                            action: step.action.clone(),
                            description: step.description.clone(),
                            evidence: Vec::new(),
                            error: Some(format!("Unknown action {}", step.action)),
                        });
                    }
                }
            })
            .await;
            if timed.is_err() {
                obs.push(Observation {
                    step_id: step.id.clone(),
                    action: step.action.clone(),
                    description: step.description.clone(),
                    evidence: Vec::new(),
                    error: Some("step timed out".to_string()),
                });
                steps_run += 1;
                continue;
            }
            steps_run += 1;
        }
        // If no evidence was collected, append a not-found observation to trigger explicit handling.
        if (total_lines == 0 || coverage_summary.total_hits() == 0)
            && !obs.iter().any(|o| o.action == "final_answer_not_found")
        {
            obs.push(Observation {
                step_id: format!("nf{}", steps_run + 1),
                action: "final_answer_not_found".to_string(),
                description: "No evidence collected; report not found".to_string(),
                evidence: Vec::new(),
                error: None,
            });
            coverage_notes.push("auto-final_answer_not_found (no evidence)".to_string());
        }
        let coverage = format!(
            "steps_run: {}, actions: {}, observations: {}, evidence_lines: {}",
            steps_run,
            actions.join(", "),
            obs.len(),
            total_lines
        );
        ExecutionResult {
            observations: obs,
            actions_run: actions,
            coverage,
            total_evidence_lines: total_lines,
            coverage_notes,
            coverage_summary,
        }
    }
}

fn render_graph(sub: &GraphSubgraph) -> String {
    let mut out = String::new();
    out.push_str("Nodes:\n");
    for n in &sub.nodes {
        out.push_str(&format!(" - {} ({})\n", n.id, n.label));
    }
    out.push_str("Edges:\n");
    for e in &sub.edges {
        out.push_str(&format!(" - {} -{}-> {}\n", e.source, e.kind, e.target));
    }
    out
}
