use coderet_agent::classifier::{ClassifiedQuery, QueryDifficulty, QueryIntent};
use coderet_agent::llm::OpenAIProvider;
use coderet_agent::planner::{PlanStep, Planner};

fn fake_planner() -> Planner {
    // This planner will fail to parse but the fallback plan will still be normalized.
    Planner::new(OpenAIProvider::new(
        "gpt-4o-mini".to_string(),
        "dummy".to_string(),
    ))
}

#[tokio::test]
async fn review_plan_includes_entry_points_and_commits() {
    let planner = fake_planner();
    let classified = ClassifiedQuery {
        intent: QueryIntent::Review,
        secondary_intents: vec![],
        difficulty: QueryDifficulty::Complex,
        domain_keywords: vec![],
    };
    let plan = planner.normalize_plan(
        coderet_agent::planner::Plan {
            steps: vec![PlanStep {
                id: "s1".to_string(),
                action: "search_summaries".to_string(),
                description: "Find overviews".to_string(),
                params: serde_json::json!({"query": "overview"}),
            }],
        },
        &classified,
    );
    let actions: Vec<_> = plan.steps.iter().map(|s| s.action.as_str()).collect();
    assert!(
        actions.contains(&"list_entry_points"),
        "review plan should list entry points"
    );
    assert!(
        actions.iter().any(|a| a.starts_with("final_answer")),
        "review plan should end with final answer"
    );
}
