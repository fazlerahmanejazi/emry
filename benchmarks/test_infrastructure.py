#!/usr/bin/env python3
"""
Quick test script to verify benchmark infrastructure without installing datasets.

Uses mock data to test all components.
"""

import sys
import time
from pathlib import Path

# Add parent to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from benchmarks.tracker import BenchmarkTracker
from benchmarks.metrics import compute_all_metrics, compute_latency_stats
from benchmarks.wrapper import MockRetriever


def test_smoke():
    """Test with mock data (no datasets required)."""
    print("\n🧪 Testing benchmark infrastructure with mock data...")
    
    # Create mock queries and labels
    queries = [f"Sample query {i}" for i in range(10)]
    relevance_labels = [[f"doc_{i}"] for i in range(10)]
    
    # Initialize tracker
    tracker = BenchmarkTracker(results_dir="benchmarks/results", use_wandb=False)
    
    # Initialize mock retriever
    retriever = MockRetriever()
    
    # Run queries
    all_results = []
    all_latencies = []
    start_time = time.time()
    
    print(f"Running {len(queries)} mock queries...")
    
    for i, (query, relevant_docs) in enumerate(zip(queries, relevance_labels)):
        query_start = time.time()
        
        # Retrieve results
        results = retriever.search([query], top_k=10)[0]
        
        query_latency = (time.time() - query_start) * 1000  # ms
        all_latencies.append(query_latency)
        
        # Build relevance list
        retrieved_ids = [r['doc_id'] for r in results]
        relevances = [1 if doc_id in relevant_docs else 0 for doc_id in retrieved_ids]
        
        # Store result
        query_result = {
            "query_id": f"mock_{i}",
            "query": query,
            "dataset": "mock",
            "latency_ms": query_latency,
            "relevances": relevances,
            "num_relevant": sum(relevances),
        }
        all_results.append(query_result)
        tracker.log_query(query_result)
    
    # Compute metrics
    total_duration = time.time() - start_time
    metrics = compute_all_metrics(all_results, k_values=[5, 10])
    metrics.update(compute_latency_stats(all_latencies))
    
    # Log run
    run_summary = {
        "config": "smoke_test",
        "datasets": ["mock"],
        "num_queries": len(all_results),
        "duration_sec": total_duration,
        "metrics": metrics,
    }
    tracker.log_run(run_summary)
    
    # Print summary
    tracker.print_summary(metrics, len(all_results), total_duration)
    tracker.finish()
    
    print("\n✅ Test passed! Benchmark infrastructure is working correctly.")
    print(f"📁 Check results at: benchmarks/results/{tracker.run_id}*")
    
    return True


if __name__ == "__main__":
    test_smoke()
