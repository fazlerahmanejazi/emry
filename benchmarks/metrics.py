"""
Metric computation utilities for code retrieval benchmarks.

Implements standard IR metrics: NDCG, MAP, MRR, Recall, Precision.
"""

import numpy as np
from typing import List, Dict, Any


def dcg_at_k(relevances: List[float], k: int) -> float:
    """Compute Discounted Cumulative Gain at k."""
    relevances = np.array(relevances[:k])
    if len(relevances) == 0:
        return 0.0
    # DCG = sum(rel_i / log2(i + 2)) for i in range(k)
    discounts = np.log2(np.arange(2, len(relevances) + 2))
    return np.sum(relevances / discounts)


def ndcg_at_k(relevances: List[float], k: int) -> float:
    """Compute Normalized Discounted Cumulative Gain at k."""
    dcg = dcg_at_k(relevances, k)
    # Ideal DCG: sort relevances in descending order
    ideal_relevances = sorted(relevances, reverse=True)
    idcg = dcg_at_k(ideal_relevances, k)
    
    if idcg == 0:
        return 0.0
    return dcg / idcg


def recall_at_k(relevances: List[int], k: int) -> float:
    """Compute Recall@k."""
    total_relevant = sum(relevances)
    if total_relevant == 0:
        return 0.0
    
    relevant_at_k = sum(relevances[:k])
    return relevant_at_k / total_relevant


def precision_at_k(relevances: List[int], k: int) -> float:
    """Compute Precision@k."""
    if k == 0:
        return 0.0
    relevant_at_k = sum(relevances[:k])
    return relevant_at_k / k


def average_precision(relevances: List[int]) -> float:
    """Compute Average Precision for a single query."""
    if sum(relevances) == 0:
        return 0.0
    
    precisions = []
    for k in range(1, len(relevances) + 1):
        if relevances[k - 1] == 1:  # If k-th result is relevant
            precisions.append(precision_at_k(relevances, k))
    
    if len(precisions) == 0:
        return 0.0
    return sum(precisions) / len(precisions)


def mean_average_precision(all_relevances: List[List[int]]) -> float:
    """Compute MAP across all queries."""
    if len(all_relevances) == 0:
        return 0.0
    aps = [average_precision(rels) for rels in all_relevances]
    return np.mean(aps)


def reciprocal_rank(relevances: List[int]) -> float:
    """Compute Reciprocal Rank (1 / rank of first relevant)."""
    for i, rel in enumerate(relevances):
        if rel == 1:
            return 1.0 / (i + 1)
    return 0.0


def mean_reciprocal_rank(all_relevances: List[List[int]]) -> float:
    """Compute MRR across all queries."""
    if len(all_relevances) == 0:
        return 0.0
    rrs = [reciprocal_rank(rels) for rels in all_relevances]
    return np.mean(rrs)


def compute_all_metrics(results: List[Dict[str, Any]], k_values: List[int] = [5, 10]) -> Dict[str, float]:
    """
    Compute all metrics for a list of query results.
    
    Args:
        results: List of dicts with 'relevances' key (list of 0/1 or floats)
        k_values: K values for @k metrics
    
    Returns:
        Dictionary of metric name -> value
    """
    all_relevances = [r['relevances'] for r in results]
    
    metrics = {}
    
    # NDCG@k
    for k in k_values:
        ndcgs = [ndcg_at_k(rels, k) for rels in all_relevances]
        metrics[f'ndcg@{k}'] = np.mean(ndcgs)
    
    # Recall@k
    for k in k_values:
        # Convert to binary if needed
        binary_rels = [[1 if r > 0 else 0 for r in rels] for rels in all_relevances]
        recalls = [recall_at_k(rels, k) for rels in binary_rels]
        metrics[f'recall@{k}'] = np.mean(recalls)
    
    # Precision@k
    for k in k_values:
        binary_rels = [[1 if r > 0 else 0 for r in rels] for rels in all_relevances]
        precisions = [precision_at_k(rels, k) for rels in binary_rels]
        metrics[f'precision@{k}'] = np.mean(precisions)
    
    # MAP
    binary_rels = [[1 if r > 0 else 0 for r in rels] for rels in all_relevances]
    metrics['map'] = mean_average_precision(binary_rels)
    
    # MRR
    metrics['mrr'] = mean_reciprocal_rank(binary_rels)
    
    return metrics


def compute_latency_stats(latencies: List[float]) -> Dict[str, float]:
    """Compute latency statistics (P50, P95, P99, avg)."""
    if len(latencies) == 0:
        return {"p50": 0, "p95": 0, "p99": 0, "avg": 0}
    
    return {
        "p50_latency": float(np.percentile(latencies, 50)),
        "p95_latency": float(np.percentile(latencies, 95)),
        "p99_latency": float(np.percentile(latencies, 99)),
        "avg_latency": float(np.mean(latencies)),
    }
