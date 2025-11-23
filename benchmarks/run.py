"""
Main benchmark runner.

Usage:
    python benchmarks/run.py --config smoke
    python benchmarks/run.py --config dev --wandb
"""

import argparse
import time
from pathlib import Path
import sys

# Add parent to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from benchmarks.config import BENCHMARK_CONFIGS, WANDB_CONFIG, METRICS
from benchmarks.tracker import BenchmarkTracker
from benchmarks.metrics import compute_all_metrics, compute_latency_stats
from benchmarks.wrapper import MockRetriever, SimpleCorpusRetriever, RealCodeRetriever
from benchmarks.benchmark_datasets.loader import load_dataset_by_name


def run_benchmark(config_name: str, use_wandb: bool = False):
    """Run a benchmark with the specified configuration."""
    
    # Load config
    if config_name not in BENCHMARK_CONFIGS:
        print(f"ERROR: Unknown config '{config_name}'")
        print(f"Available configs: {list(BENCHMARK_CONFIGS.keys())}")
        return
    
    config = BENCHMARK_CONFIGS[config_name]
    print(f"\n🚀 Running benchmark: {config['name']}")
    print(f"   Purpose: {config['purpose']}")
    print(f"   Cost estimate: {config['cost_estimate']}")
    print(f"   Samples per dataset: {config['samples']}")
    print()
    
    # Initialize tracker
    wandb_cfg = WANDB_CONFIG.copy() if use_wandb else None
    if wandb_cfg:
        wandb_cfg["run_config"] = {"benchmark_config": config_name}
    
    tracker = BenchmarkTracker(
        results_dir="benchmarks/results",
        use_wandb=use_wandb,
        wandb_config=wandb_cfg,
    )
    
    # Run benchmarks for each dataset
    all_results = []
    all_latencies = []
    start_time = time.time()
    
    for dataset_name in config['datasets']:
        print(f"📊 Loading dataset: {dataset_name}")
        
        try:
            queries, relevance_labels, corpus = load_dataset_by_name(
                dataset_name,
                max_samples=config['samples']
            )
            print(f"   Loaded {len(queries)} queries")
        except Exception as e:
            print(f"   ERROR loading dataset: {e}")
            continue
        
        if len(queries) == 0:
            print(f"   WARNING: No queries loaded, skipping")
            continue
        
        # Initialize retriever with the corpus for THIS dataset
        retriever = SimpleCorpusRetriever(corpus)
        print(f"   Initialized retriever with {len(corpus)} documents")
        
        # Run queries
        print(f"   Running queries...")
        for i, (query, relevant_docs) in enumerate(zip(queries, relevance_labels)):
            query_start = time.time()
            
            # Retrieve results
            results = retriever.search([query], top_k=10)[0]
            
            query_latency = (time.time() - query_start) * 1000  # ms
            all_latencies.append(query_latency)
            
            # Build relevance list (1 if doc is relevant, 0 otherwise)
            retrieved_ids = [r['doc_id'] for r in results]
            relevances = [1 if doc_id in relevant_docs else 0 for doc_id in retrieved_ids]
            
            # Store result
            query_result = {
                "query_id": f"{dataset_name}_{i}",
                "query": query,
                "dataset": dataset_name,
                "latency_ms": query_latency,
                "relevances": relevances,
                "num_relevant": sum(relevances),
            }
            all_results.append(query_result)
            tracker.log_query(query_result)
            
            # Progress indicator
            if (i + 1) % 10 == 0:
                print(f"   Progress: {i+1}/{len(queries)}")
        
        print(f"   ✓ Complete: {len(queries)} queries processed")
    
    # Compute aggregate metrics
    total_duration = time.time() - start_time
    
    metrics = {}
    if all_results:
        metrics.update(compute_all_metrics(all_results, k_values=[5, 10]))
    if all_latencies:
        metrics.update(compute_latency_stats(all_latencies))
    
    # Log run summary
    run_summary = {
        "config": config_name,
        "datasets": config['datasets'],
        "num_queries": len(all_results),
        "duration_sec": total_duration,
        "metrics": metrics,
    }
    tracker.log_run(run_summary)
    
    # Log dataset aggregate
    for dataset_name in config['datasets']:
        tracker.log_aggregate(dataset_name, metrics)
    
    # Print summary
    tracker.print_summary(metrics, len(all_results), total_duration)
    
    # Finish tracking
    tracker.finish()
    
    return metrics


def main():
    parser = argparse.ArgumentParser(description="Run code-retriever benchmarks")
    parser.add_argument(
        "--config",
        type=str,
        default="smoke",
        choices=list(BENCHMARK_CONFIGS.keys()),
        help="Benchmark configuration to run"
    )
    parser.add_argument(
        "--wandb",
        action="store_true",
        help="Enable Weights & Biases tracking"
    )
    
    args = parser.parse_args()
    
    run_benchmark(args.config, use_wandb=args.wandb)


if __name__ == "__main__":
    main()
