"""
Dataset loader for code retrieval benchmarks.

Supports:
- CoNaLa (Code/Natural Language) - Python code generation from NL
- JSON format datasets (local files)
"""

import sys
import json
from pathlib import Path
from typing import List, Dict, Any, Tuple

try:
    from datasets import load_dataset
    DATASETS_AVAILABLE = True
except ImportError:
    DATASETS_AVAILABLE = False
    print("WARNING: datasets package not installed. Run: pip install datasets")


def load_json_dataset(filepath: str) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """Load dataset from JSON file in our format."""
    filepath = Path(filepath)
    
    if not filepath.exists():
        print(f"   ERROR: File not found: {filepath}")
        return [], [], {}
    
    with open(filepath) as f:
        data = json.load(f)
    
    queries = data.get('queries', [])
    relevance_labels = data.get('relevance_labels', [[f"code_{i}"] for i in range(len(queries))])
    corpus = data.get('corpus', {})
    
    return queries, relevance_labels, corpus


def load_conala(split: str = "test", max_samples: int = None) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """
    Load the CoNaLa dataset (Code/Natural Language).
    
    Returns:
        queries: List of natural language queries
        relevance_labels: List of relevant doc IDs per query
        corpus: Dict mapping doc_id -> code snippet
    """
    if not DATASETS_AVAILABLE:
        return [], [], {}
    
    try:
        # Load CoNaLa from HuggingFace (use processed version to avoid deprecated scripts)
        print(f"   Downloading CoNaLa dataset...")
        dataset = load_dataset("AhmedSSoliman/CoNaLa", split="train")  # Only has train split
        print(f"   Loaded {len(dataset)} examples from CoNaLa")
    except Exception as e:
        print(f"   ERROR: Could not load CoNaLa dataset: {e}")
        return [], [], {}
    
    if max_samples:
        dataset = dataset.select(range(min(max_samples, len(dataset))))
    
    queries = []
    relevance_labels = []
    corpus = {}
    
    for i, example in enumerate(dataset):
        # Use rewritten_intent if available, otherwise use intent
        query = example.get("rewritten_intent", "") or example.get("intent", "")
        
        if query and len(query.strip()) > 5:  # Filter very short queries
            queries.append(query)
            
            # The snippet is the relevant "document"
            doc_id = f"conala_{i}"
            snippet = example.get("snippet", "")
            corpus[doc_id] = snippet
            relevance_labels.append([doc_id])
    
    return queries, relevance_labels, corpus


def load_curated_test() -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """Load the curated test dataset (JSON format)."""
    dataset_path = Path(__file__).parent / "curated_test.json"
    return load_json_dataset(str(dataset_path))


def load_cosqa(split: str = "test", max_samples: int = None) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """
    Alias - loads curated test set instead of deprecated CodeSearchNet.
    """
    return load_curated_test()


def load_codesearchnet_python(split: str = "test", max_samples: int = None) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """
    Alias - loads curated test set instead of deprecated CodeSearchNet.
    """
    return load_curated_test()


def load_dataset_by_name(name: str, max_samples: int = None) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """
    Load a dataset by name.
    
    Args:
        name: Dataset name ('cosqa', 'codesearchnet_python', 'conala', 'curated')
        max_samples: Max number of samples to load
    
    Returns:
        queries, relevance_labels, corpus
    """
    if name in ["cosqa", "codesearchnet_python", "curated"]:
        return load_curated_test()
    elif name == "conala":
        return load_conala(max_samples=max_samples)
    else:
        raise ValueError(f"Unknown dataset: {name}. Supported: cosqa, codesearchnet_python, conala, curated")
