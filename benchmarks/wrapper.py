"""
Wrapper for code-retriever to make it compatible with CoIR/BEIR evaluation frameworks.
"""

import sys
import time
from pathlib import Path
from typing import List, Dict, Any
import subprocess
import json


class CodeRetrieverWrapper:
    """
    Wrapper for code-retriever CLI to work with benchmark frameworks.
    
    This wraps the `ask` command and formats results for evaluation.
    """
    
    def __init__(self, index_path: str = ".codeindex", binary_path: str = None):
        """
        Args:
            index_path: Path to the code index
            binary_path: Path to coderet binary (default: cargo run --)
        """
        self.index_path = Path(index_path)
        self.binary_path = binary_path
        
        if not self.index_path.exists():
            print(f"WARNING: Index not found at {self.index_path}")
            print("Run 'cargo run -- index' first to create an index.")
    
    def search(self, queries: List[str], top_k: int = 10) -> List[List[Dict[str, Any]]]:
        """
        Search for multiple queries.
        
        Args:
            queries: List of natural language queries
            top_k: Number of results to return per query
        
        Returns:
            List of result lists, one per query.
            Each result is a dict with 'doc_id', 'score', 'content'
        """
        all_results = []
        
        for query in queries:
            results = self._search_single(query, top_k)
            all_results.append(results)
        
        return all_results
    
    def _search_single(self, query: str, top_k: int = 10) -> List[Dict[str, Any]]:
        """
        Search for a single query using code-retriever.
        
        Note: This is a simplified implementation. For full eval, you'd want to:
        1. Use the Rust API directly (faster)
        2. Or parse structured output from CLI
        """
        # For now, return empty results as placeholder
        # TODO: Integrate with actual retriever
        return []
    
    def get_corpus(self) -> Dict[str, str]:
        """
        Get the indexed corpus.
        
        Returns:
            Dict mapping doc_id -> document text
        """
        # TODO: Load from index
        return {}


class MockRetriever:
    """Mock retriever for testing the benchmark infrastructure."""
    
    def search(self, queries: List[str], top_k: int = 10) -> List[List[Dict[str, Any]]]:
        """Return mock results for testing."""
        import random
        
        all_results = []
        for query in queries:
            results = []
            for i in range(top_k):
                results.append({
                    "doc_id": f"doc_{i}",
                    "score": random.random(),
                    "content": f"Mock result {i} for: {query[:30]}..."
                })
            all_results.append(results)
        
        return all_results


class SimpleCorpusRetriever:
    """
    Simple retriever that searches a given corpus using keyword matching.
    
    This is a baseline that actually returns relevant results!
    """
    
    def __init__(self, corpus: Dict[str, str]):
        """
        Args:
            corpus: Dict mapping doc_id -> document text
        """
        self.corpus = corpus
    
    def search(self, queries: List[str], top_k: int = 10) -> List[List[Dict[str, Any]]]:
        """Search using simple keyword matching."""
        all_results = []
        
        for query in queries:
            results = self._search_single(query, top_k)
            all_results.append(results)
        
        return all_results
    
    def _search_single(self, query: str, top_k: int = 10) -> List[Dict[str, Any]]:
        """
        Simple keyword-based search.
        
        Scores documents by counting matching words between query and document.
        """
        query_words = set(query.lower().split())
        
        scores = []
        for doc_id, content in self.corpus.items():
            doc_words = set(content.lower().split())
            
            # Simple overlap score (number of matching words)
            overlap = len(query_words & doc_words)
            
            # Normalize by query length
            score = overlap / len(query_words) if query_words else 0
            
            scores.append({
                "doc_id": doc_id,
                "score": score,
                "content": content[:200]  # First 200 chars
            })
        
        # Sort by score (descending) and return top_k
        scores.sort(key=lambda x: x['score'], reverse=True)
        return scores[:top_k]

class RealCodeRetriever:
    """
    Integration with the actual code-retriever project!
    
    Uses the Rust core library via Python bindings or subprocess.
    """
    
    def __init__(self, index_path: str = None):
        """
        Args:
            index_path: Path to the index directory (default: auto-detect)
        """
        # Try to import the core library
        try:
            sys.path.insert(0, str(Path(__file__).parent.parent))
            from core.retriever import Retriever
            from core.config import Config, SearchMode
            
            self.use_python_api = True
            
            # Load config
            if index_path:
                # TODO: Load config with custom index path
                self.config = Config.load_default()
            else:
                self.config = Config.load_default()
            
            # Initialize retriever (needs async context)
            # We'll do this lazily in search()
            self.retriever = None
            self.config_obj = self.config
            
        except ImportError:
            print("   WARNING: Could not import core library")
            print("   Falling back to CLI-based search")
            self.use_python_api = False
            self.index_path = index_path or Path.cwd() / ".codeindex"
    
    def search(self, queries: List[str], top_k: int = 10) -> List[List[Dict[str, Any]]]:
        """Search using the real code-retriever."""
        
        if self.use_python_api:
            return self._search_via_python(queries, top_k)
        else:
            return self._search_via_cli(queries, top_k)
    
    def _search_via_python(self, queries: List[str], top_k: int) -> List[List[Dict[str, Any]]]:
        """Search using Python API (if available)."""
        # Since the retriever is async, we need to use asyncio
        import asyncio
        
        async def search_async():
            from core.retriever import Retriever
            from core.config import SearchMode
            
            if self.retriever is None:
                self.retriever = await Retriever.create(self.config_obj)
            
            all_results = []
            for query in queries:
                # Search with hybrid mode
                search_results = await self.retriever.search(query, SearchMode.Hybrid, top_k)
                
                # Convert to benchmark format
                results = []
                for res in search_results:
                    results.append({
                        "doc_id": str(res.chunk.file_path),  # Use file path as ID
                        "score": float(res.score),
                        "content": res.chunk.content[:200]  # First 200 chars
                    })
                all_results.append(results)
            
            return all_results
        
        # Run async function
        try:
            loop = asyncio.get_event_loop()
        except RuntimeError:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
        
        return loop.run_until_complete(search_async())
    
    def _search_via_cli(self, queries: List[str], top_k: int) -> List[List[Dict[str, Any]]]:
        """Search using CLI (fallback)."""
        import subprocess
        import json
        
        all_results = []
        
        for query in queries:
            try:
                # Run the ask command
                cmd = ["cargo", "run", "--release", "--", "ask", query, "--top", str(top_k)]
                result = subprocess.run(
                    cmd,
                    cwd=Path(__file__).parent.parent,
                    capture_output=True,
                    text=True,
                    timeout=30
                )
                
                # Parse output (this is simplified - you'd need to parse actual output)
                # For now, return empty since CLI doesn't return structured data
                all_results.append([])
                
            except Exception as e:
                print(f"   ERROR running CLI search: {e}")
                all_results.append([])
        
        return all_results
