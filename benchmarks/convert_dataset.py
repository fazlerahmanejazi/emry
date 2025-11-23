"""
Convert various dataset formats to our benchmark format.

Supports:
- HuggingFace datasets (with deprecated scripts)
- JSON files
- CSV files
- Custom formats
"""

import json
from pathlib import Path
from typing import List, Dict, Tuple


def convert_conala_dict_to_format(data: List[Dict]) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """
    Convert CoNaLa-style dict data to our format.
    
    Expected fields: 'intent' or 'rewritten_intent', 'snippet'
    """
    queries = []
    relevance_labels = []
    corpus = {}
    
    for i, item in enumerate(data):
        # Get query
        query = item.get('rewritten_intent') or item.get('intent') or item.get('text', '')
        
        # Get code snippet
        snippet = item.get('snippet') or item.get('target') or item.get('code', '')
        
        if query and snippet and len(query.strip()) > 5:
            queries.append(query.strip())
            
            doc_id = f"doc_{i}"
            corpus[doc_id] = snippet
            relevance_labels.append([doc_id])
    
    return queries, relevance_labels, corpus


def load_json_dataset(filepath: str) -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """Load dataset from JSON file."""
    with open(filepath) as f:
        data = json.load(f)
    
    # Handle different JSON structures
    if isinstance(data, list):
        return convert_conala_dict_to_format(data)
    elif isinstance(data, dict):
        # Check if it's already in our format
        if 'queries' in data and 'corpus' in data:
            return data['queries'], data.get('relevance_labels', []), data['corpus']
        # Otherwise treat as single example
        return convert_conala_dict_to_format([data])
    
    return [], [], {}


def create_curated_test_set() -> Tuple[List[str], List[List[str]], Dict[str, str]]:
    """
    Create a small curated test set based on common code search queries.
    
    These are inspired by the CodeSearchNet Challenge paper.
    """
    test_data = [
        {
            "query": "Convert string to integer",
            "code": "def string_to_int(s):\n    try:\n        return int(s)\n    except ValueError:\n        return None"
        },
        {
            "query": "Read file contents",
            "code": "def read_file(filepath):\n    with open(filepath, 'r') as f:\n        return f.read()"
        },
        {
            "query": "Check if string is valid email",
            "code": "import re\n\ndef is_valid_email(email):\n    pattern = r'^[\\w\\.-]+@[\\w\\.-]+\\.\\w+$'\n    return bool(re.match(pattern, email))"
        },
        {
            "query": "Convert list to dictionary",
            "code": "def list_to_dict(items, key_func):\n    return {key_func(item): item for item in items}"
        },
        {
            "query": "Remove duplicates from list",
            "code": "def remove_duplicates(lst):\n    return list(set(lst))"
        },
        {
            "query": "Sort dictionary by value",
            "code": "def sort_dict_by_value(d, reverse=False):\n    return dict(sorted(d.items(), key=lambda x: x[1], reverse=reverse))"
        },
        {
            "query": "Get current timestamp",
            "code": "import time\n\ndef get_timestamp():\n    return int(time.time())"
        },
        {
            "query": "Parse JSON string",
            "code": "import json\n\ndef parse_json(json_string):\n    try:\n        return json.loads(json_string)\n    except json.JSONDecodeError:\n        return None"
        },
        {
            "query": "Calculate file size",
            "code": "import os\n\ndef get_file_size(filepath):\n    return os.path.getsize(filepath)"
        },
        {
            "query": "Create directory if not exists",
            "code": "import os\n\ndef mkdir_if_not_exists(path):\n    os.makedirs(path, exist_ok=True)"
        },
    ]
    
    queries = [item['query'] for item in test_data]
    corpus = {f"code_{i}": item['code'] for i, item in enumerate(test_data)}
    relevance_labels = [[f"code_{i}"] for i in range(len(test_data))]
    
    return queries, relevance_labels, corpus


def save_dataset(queries, relevance_labels, corpus, output_path):
    """Save dataset in our internal JSON format."""
    data = {
        "queries": queries,
        "relevance_labels": relevance_labels,
        "corpus": corpus
    }
    
    with open(output_path, 'w') as f:
        json.dump(data, f, indent=2)
    
    print(f"Saved dataset to {output_path}")
    print(f"  Queries: {len(queries)}")
    print(f"  Corpus size: {len(corpus)}")


if __name__ == "__main__":
    # Create and save curated test set
    queries, labels, corpus = create_curated_test_set()
    save_dataset(queries, labels, corpus, "benchmark_datasets/curated_test.json")
