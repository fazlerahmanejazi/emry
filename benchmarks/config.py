"""
Benchmark configuration for code-retriever evaluation.

Defines different benchmark configs with varying sample sizes and datasets.
"""

BENCHMARK_CONFIGS = {
    "smoke": {
        "name": "Smoke Test",
        "samples": 10,
        "datasets": ["cosqa"],
        "purpose": "Quick sanity check (5 min)",
        "cost_estimate": "$0",
    },
    "dev": {
        "name": "Development",
        "samples": 100,
        "datasets": ["cosqa", "codesearchnet_python"],
        "purpose": "Development & iteration (30 min)",
        "cost_estimate": "$0-1 (with OpenAI)",
    },
    "validation": {
        "name": "Validation",
        "samples": 500,
        "datasets": ["cosqa", "codesearchnet_python"],
        "purpose": "Pre-release validation (2-3 hours)",
        "cost_estimate": "$2-5",
    },
    "full": {
        "name": "Full Evaluation",
        "samples": None,  # Use full test set
        "datasets": ["cosqa", "codesearchnet_python"],
        "purpose": "Complete evaluation for papers (8-12 hours)",
        "cost_estimate": "$10-20",
    },
}

# Metrics to compute
METRICS = {
    "search_quality": [
        "ndcg@5",
        "ndcg@10",
        "map",
        "recall@5",
        "recall@10",
        "mrr",
        "precision@5",
    ],
    "performance": [
        "p50_latency",
        "p95_latency",
        "p99_latency",
        "avg_latency",
    ],
    "agent": [
        "tool_selection_accuracy",
        "avg_tools_per_query",
        "tool_error_rate",
    ],
}

# W&B configuration
WANDB_CONFIG = {
    "project": "code-retriever-benchmarks",
    "entity": None,  # Set to your W&B username/team
    "tags": ["evaluation", "code-search"],
}
