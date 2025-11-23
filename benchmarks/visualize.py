"""
Beautiful local visualizations for benchmark results.

No cloud account needed - generates HTML dashboard with charts.
"""

import json
import sys
from pathlib import Path
from datetime import datetime
from typing import Dict, List, Any

try:
    import matplotlib.pyplot as plt
    import matplotlib
    matplotlib.use('Agg')  # Non-interactive backend
    MATPLOTLIB_AVAILABLE = True
except ImportError:
    MATPLOTLIB_AVAILABLE = False
    print("ERROR: matplotlib not installed. Run: pip install matplotlib")
    sys.exit(1)

import numpy as np


def load_latest_run(results_dir: str = "results") -> Dict[str, Any]:
    """Load the most recent run summary."""
    results_path = Path(results_dir)
    
    summaries = list(results_path.glob("*_summary.json"))
    if not summaries:
        print(f"No results found in {results_dir}")
        return None
    
    # Get latest
    latest = max(summaries, key=lambda p: p.stat().st_mtime)
    
    with open(latest) as f:
        return json.load(f)


def load_history(dataset_name: str, results_dir: str = "results") -> List[Dict]:
    """Load historical results for a dataset."""
    history_file = Path(results_dir) / f"{dataset_name}_history.jsonl"
    
    if not history_file.exists():
        return []
    
    history = []
    with open(history_file) as f:
        for line in f:
            history.append(json.loads(line))
    
    return history


def create_metrics_chart(metrics: Dict[str, float], output_file: str):
    """Create a bar chart of metrics."""
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 5))
    
    # Search quality metrics
    search_metrics = {
        'NDCG@5': metrics.get('ndcg@5', 0),
        'NDCG@10': metrics.get('ndcg@10', 0),
        'Recall@5': metrics.get('recall@5', 0),
        'Recall@10': metrics.get('recall@10', 0),
        'MAP': metrics.get('map', 0),
        'MRR': metrics.get('mrr', 0),
    }
    
    ax1.barh(list(search_metrics.keys()), list(search_metrics.values()),
             color='#4CAF50', alpha=0.8)
    ax1.set_xlabel('Score', fontsize=12)
    ax1.set_title('Search Quality Metrics', fontsize=14, fontweight='bold')
    ax1.set_xlim(0, 1)
    ax1.grid(axis='x', alpha=0.3)
    
    # Add value labels
    for i, (metric, value) in enumerate(search_metrics.items()):
        ax1.text(value + 0.02, i, f'{value:.3f}', va='center', fontsize=10)
    
    # Performance metrics
    perf_metrics = {
        'P50': metrics.get('p50_latency', 0),
        'P95': metrics.get('p95_latency', 0),
        'P99': metrics.get('p99_latency', 0),
    }
    
    ax2.bar(list(perf_metrics.keys()), list(perf_metrics.values()),
            color='#2196F3', alpha=0.8)
    ax2.set_ylabel('Latency (ms)', fontsize=12)
    ax2.set_title('Performance (Latency)', fontsize=14, fontweight='bold')
    ax2.grid(axis='y', alpha=0.3)
    
    # Add value labels
    for i, (metric, value) in enumerate(perf_metrics.items()):
        ax2.text(i, value + 5, f'{value:.1f}ms', ha='center', fontsize=10)
    
    plt.tight_layout()
    plt.savefig(output_file, dpi=150, bbox_inches='tight')
    plt.close()
    
    return output_file


def create_trend_chart(history: List[Dict], output_file: str):
    """Create a time series chart of metrics over time."""
    if len(history) < 2:
        return None
    
    # Extract data
    dates = [h['date'] for h in history]
    ndcg_10 = [h.get('ndcg@10', 0) for h in history]
    recall_10 = [h.get('recall@10', 0) for h in history]
    p95_lat = [h.get('p95_latency', 0) for h in history]
    
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(12, 8))
    
    # Accuracy over time
    x = range(len(dates))
    ax1.plot(x, ndcg_10, marker='o', label='NDCG@10', linewidth=2, color='#4CAF50')
    ax1.plot(x, recall_10, marker='s', label='Recall@10', linewidth=2, color='#2196F3')
    ax1.set_ylabel('Score', fontsize=12)
    ax1.set_title('Search Quality Trends', fontsize=14, fontweight='bold')
    ax1.legend(fontsize=11)
    ax1.grid(alpha=0.3)
    ax1.set_ylim(0, 1)
    ax1.set_xticks(x)
    ax1.set_xticklabels([d.split('_')[0] for d in dates], rotation=45, ha='right')
    
    # Latency over time
    ax2.plot(x, p95_lat, marker='D', linewidth=2, color='#FF9800')
    ax2.set_ylabel('P95 Latency (ms)', fontsize=12)
    ax2.set_xlabel('Run Date', fontsize=12)
    ax2.set_title('Performance Trends', fontsize=14, fontweight='bold')
    ax2.grid(alpha=0.3)
    ax2.set_xticks(x)
    ax2.set_xticklabels([d.split('_')[0] for d in dates], rotation=45, ha='right')
    
    plt.tight_layout()
    plt.savefig(output_file, dpi=150, bbox_inches='tight')
    plt.close()
    
    return output_file


def generate_html_dashboard(run_data: Dict, charts: Dict[str, str], output_file: str = "dashboard.html"):
    """Generate an HTML dashboard with embedded charts."""
    
    metrics = run_data.get('metrics', {})
    
    html = f"""
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Benchmark Dashboard - {run_data.get('run_id', 'Latest')}</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            max-width: 1200px;
            margin: 40px auto;
            padding: 20px;
            background: #f5f5f5;
        }}
        .header {{
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 30px;
            border-radius: 10px;
            margin-bottom: 30px;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
        }}
        .header h1 {{
            margin: 0 0 10px 0;
            font-size: 32px;
        }}
        .header p {{
            margin: 5px 0;
            opacity: 0.9;
        }}
        .metrics-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}
        .metric-card {{
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }}
        .metric-value {{
            font-size: 36px;
            font-weight: bold;
            color: #667eea;
            margin: 10px 0;
        }}
        .metric-label {{
            color: #666;
            font-size: 14px;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }}
        .chart-section {{
            background: white;
            padding: 30px;
            border-radius: 8px;
            margin-bottom: 20px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }}
        .chart-section h2 {{
            margin-top: 0;
            color: #333;
        }}
        .chart-section img {{
            max-width: 100%;
            height: auto;
        }}
        .footer {{
            text-align: center;
            color: #999;
            margin-top: 40px;
            font-size: 14px;
        }}
    </style>
</head>
<body>
    <div class="header">
        <h1>📊 Benchmark Dashboard</h1>
        <p><strong>Run ID:</strong> {run_data.get('run_id', 'N/A')}</p>
        <p><strong>Config:</strong> {run_data.get('config', 'N/A')}</p>
        <p><strong>Queries:</strong> {run_data.get('num_queries', 0)}</p>
        <p><strong>Duration:</strong> {run_data.get('duration_sec', 0):.1f}s</p>
    </div>
    
    <div class="metrics-grid">
        <div class="metric-card">
            <div class="metric-label">NDCG@10</div>
            <div class="metric-value">{metrics.get('ndcg@10', 0):.3f}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">Recall@10</div>
            <div class="metric-value">{metrics.get('recall@10', 0):.3f}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">MAP</div>
            <div class="metric-value">{metrics.get('map', 0):.3f}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">P95 Latency</div>
            <div class="metric-value">{metrics.get('p95_latency', 0):.0f}<span style="font-size: 20px;">ms</span></div>
        </div>
    </div>
    
    <div class="chart-section">
        <h2>Metrics Overview</h2>
        <img src="{charts.get('metrics', '')}" alt="Metrics Chart">
    </div>
    
    {f'''<div class="chart-section">
        <h2>Trends Over Time</h2>
        <img src="{charts.get('trends', '')}" alt="Trend Chart">
    </div>''' if charts.get('trends') else ''}
    
    <div class="footer">
        Generated on {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}
    </div>
</body>
</html>
"""
    
    with open(output_file, 'w') as f:
        f.write(html)
    
    return output_file


def main():
    """Generate visualization dashboard."""
    print("\n🎨 Generating benchmark visualization...")
    
    if not MATPLOTLIB_AVAILABLE:
        return
    
    # Load latest run
    run_data = load_latest_run()
    if not run_data:
        print("No benchmark results found. Run a benchmark first:")
        print("  python run.py --config smoke")
        return
    
    print(f"   Found run: {run_data.get('run_id')}")
    
    # Create output directory
    viz_dir = Path("visualizations")
    viz_dir.mkdir(exist_ok=True)
    
    # Generate charts
    charts = {}
    
    print("   Creating metrics chart...")
    metrics_chart = viz_dir / "metrics.png"
    create_metrics_chart(run_data.get('metrics', {}), str(metrics_chart))
    charts['metrics'] = "visualizations/metrics.png"
    
    # Try to create trends chart
    datasets = run_data.get('datasets', [])
    if datasets:
        history = load_history(datasets[0])
        if len(history) >= 2:
            print("   Creating trends chart...")
            trends_chart = viz_dir / "trends.png"
            if create_trend_chart(history, str(trends_chart)):
                charts['trends'] = "visualizations/trends.png"
    
    # Generate HTML dashboard
    print("   Creating HTML dashboard...")
    dashboard_file = viz_dir / "dashboard.html"
    generate_html_dashboard(run_data, charts, str(dashboard_file))
    
    print(f"\n✅ Dashboard generated!")
    print(f"   📁 Open in browser: {dashboard_file.absolute()}")
    print(f"   📊 Charts saved to: {viz_dir.absolute()}/")
    
    # Try to open in browser
    import webbrowser
    try:
        webbrowser.open(f"file://{dashboard_file.absolute()}")
        print(f"\n🌐 Opening dashboard in browser...")
    except:
        pass


if __name__ == "__main__":
    main()
