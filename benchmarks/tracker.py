"""
Multi-level benchmark tracking with file-based storage and optional W&B integration.

Tracks at 3 levels:
1. Individual queries (detailed)
2. Run summaries (aggregated)
3. Dataset history (over time)
"""

import json
from datetime import datetime
from pathlib import Path
from typing import Dict, Any, Optional, List
import sys

# Optional W&B import
try:
    import wandb
    WANDB_AVAILABLE = True
except ImportError:
    WANDB_AVAILABLE = False
    wandb = None


class BenchmarkTracker:
    """Multi-level benchmark result tracker with optional W&B integration."""
    
    def __init__(
        self,
        results_dir: str = "benchmarks/results",
        use_wandb: bool = False,
        wandb_config: Optional[Dict] = None,
    ):
        self.results_dir = Path(results_dir)
        self.results_dir.mkdir(parents=True, exist_ok=True)
        
        self.run_id = datetime.now().strftime("%Y-%m-%d_%H%M%S")
        self.use_wandb = use_wandb and WANDB_AVAILABLE
        
        # Storage for aggregation
        self.query_results: List[Dict] = []
        self.latencies: List[float] = []
        
        #Initialize W&B if requested
        if self.use_wandb:
            if not WANDB_AVAILABLE:
                print("WARNING: wandb not installed. Install with: pip install wandb")
                print("Falling back to file-based tracking only.")
                self.use_wandb = False
            else:
                config = wandb_config or {}
                self.wandb_run = wandb.init(
                    project=config.get("project", "code-retriever-benchmarks"),
                    name=self.run_id,
                    tags=config.get("tags", []),
                    config=config.get("run_config", {}),
                )
                print(f"✓ W&B tracking enabled: {self.wandb_run.url}")
    
    def log_query(self, query_result: Dict[str, Any]):
        """Log a single query result (Level 1: Individual query)."""
        # Add timestamp if not present
        if "timestamp" not in query_result:
            query_result["timestamp"] = datetime.now().isoformat()
        
        # Store for aggregation
        self.query_results.append(query_result)
        if "latency_ms" in query_result:
            self.latencies.append(query_result["latency_ms"])
        
        # Write to file (JSONL for queries)
        query_file = self.results_dir / f"{self.run_id}_queries.jsonl"
        with open(query_file, 'a') as f:
            f.write(json.dumps(query_result) + '\n')
        
        # Log to W&B if enabled
        if self.use_wandb:
            # Log individual query metrics
            wandb.log({
                f"query/{k}": v
                for k, v in query_result.items()
                if isinstance(v, (int, float, bool))
            })
    
    def log_run(self, run_summary: Dict[str, Any]):
        """Log run summary (Level 2: Run aggregate)."""
        run_summary["run_id"] = self.run_id
        run_summary["timestamp"] = datetime.now().isoformat()
        
        # Write to file
        run_file = self.results_dir / f"{self.run_id}_summary.json"
        with open(run_file, 'w') as f:
            json.dump(run_summary, f, indent=2)
        
        # Log to W&B
        if self.use_wandb:
            # Log all metrics as summary
            wandb.run.summary.update(run_summary.get("metrics", {}))
            
            # Create comparison table
            if "baseline_comparison" in run_summary:
                table = wandb.Table(
                    columns=["Model", "NDCG@10", "Recall@10", "MAP"],
                    data=[
                        [name, metrics.get("ndcg@10", 0), metrics.get("recall@10", 0), metrics.get("map", 0)]
                        for name, metrics in run_summary["baseline_comparison"].items()
                    ]
                )
                wandb.log({"baseline_comparison": table})
    
    def log_aggregate(self, dataset_name: str, metrics: Dict[str, float]):
        """Log dataset aggregate over time (Level 3: Time series)."""
        agg_file = self.results_dir / f"{dataset_name}_history.jsonl"
        entry = {
            "date": self.run_id,
            "timestamp": datetime.now().isoformat(),
            **metrics
        }
        with open(agg_file, 'a') as f:
            f.write(json.dumps(entry) + '\n')
    
    def finish(self):
        """Finalize tracking (close W&B run if active)."""
        if self.use_wandb and wandb.run is not None:
            wandb.finish()
    
    def print_summary(self, metrics: Dict[str, float], num_queries: int, duration_sec: float):
        """Print a formatted summary to terminal."""
        print(f"\n{'='*60}")
        print(f"  Benchmark Results - {self.run_id}")
        print(f"{'='*60}")
        
        print(f"\n📊 Search Quality:")
        for metric in ["ndcg@5", "ndcg@10", "recall@5", "recall@10", "map", "mrr"]:
            if metric in metrics:
                print(f"  {metric.upper():12} {metrics[metric]:.4f}  {self._bar(metrics[metric])}")
        
        print(f"\n⚡ Performance:")
        for metric in ["p50_latency", "p95_latency", "p99_latency"]:
            if metric in metrics:
                print(f"  {metric:12} {metrics[metric]:.1f}ms")
        
        print(f"\n✅ Queries Processed: {num_queries}")
        if num_queries > 0:
            print(f"⏱️  Total Duration:   {duration_sec:.1f}s ({duration_sec/num_queries:.2f}s per query)")
        else:
            print(f"⏱️  Total Duration:   {duration_sec:.1f}s")
            print(f"\n⚠️  No queries processed - install datasets: pip install -r requirements.txt")
        
        if self.use_wandb:
            print(f"\n🎨 W&B Dashboard:    {wandb.run.url}")
        
        print(f"📁 Results saved to: {self.results_dir / self.run_id}")
        print(f"{'='*60}\n")
    
    def _bar(self, value: float, width: int = 10) -> str:
        """Create a simple ASCII progress bar."""
        filled = int(value * width)
        return '█' * filled + '░' * (width - filled)
