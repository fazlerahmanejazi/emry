"""
CLI-based integration with code-retriever.

Calls the Rust binary and parses output for benchmarking.
"""

import subprocess
import json
import re
from pathlib import Path
from typing import List, Dict, Any


class CLICodeRetriever:
    """
    Call code-retriever via CLI and parse results.
    
    This is the simplest integration approach.
    """
    
    def __init__(self, project_root: Path = None):
        self.project_root = project_root or Path(__file__).parent.parent
        
        # Check if index exists
        index_path = self.project_root / ".codeindex"
        if not index_path.exists():
            print(f"⚠️  WARNING: No index found at {index_path}")
            print(f"   Run: cargo run -- index")
            print(f"   Before running benchmarks!")
    
    def search(self, queries: List[str], top_k: int = 10) -> List[List[Dict[str, Any]]]:
        """Search using CLI."""
        all_results = []
        
        for query in queries:
            results = self._search_single(query, top_k)
            all_results.append(results)
        
        return all_results
    
    def _search_single(self, query: str, top_k: int = 10) -> List[Dict[str, Any]]:
        """Run a single search query."""
        try:
            # Build command
            cmd = [
                "cargo", "run", "--release", "--quiet", "--",
                "ask", query,
                "--top", str(top_k)
            ]
            
            # Run command
            result = subprocess.run(
                cmd,
                cwd=self.project_root,
                capture_output=True,
                text=True,
                timeout=60  # 60s timeout
            )
            
            if result.returncode != 0:
                print(f"   ERROR: Command failed for query: {query[:50]}")
                print(f"   {result.stderr[:200]}")
                return []
            
            # Parse output
            # The CLI outputs file paths and scores, we need to extract them
            return self._parse_output(result.stdout, top_k)
            
        except subprocess.TimeoutExpired:
            print(f"   TIMEOUT: Query took >60s: {query[:50]}")
            return []
        except Exception as e:
            print(f"   ERROR: {e}")
            return []
    
    def _parse_output(self, output: str, top_k: int) -> List[Dict[str, Any]]:
        """
        Parse CLI output to extract results.
        
        Expected format (from ask command):
        - File paths in the output
        - Scores if available
        """
        results = []
        lines = output.strip().split('\n')
        
        # Look for file paths (simple heuristic)
        for i, line in enumerate(lines[:top_k * 2]):  # Look at first N lines
            # Skip agent output
            if 'Agent' in line or 'thinking' in line or 'Answer' in line:
                continue
            
            # Try to find file paths
            # This is a simple parser - you may need to adjust based on actual output
            if '.rs' in line or '.py' in line or '.md' in line:
                # Extract file path
                parts = line.strip().split()
                for part in parts:
                    if any(ext in part for ext in ['.rs', '.py', '.md', '.toml', '.json']):
                        results.append({
                            "doc_id": part,
                            "score": 1.0 - (len(results) * 0.1),  # Decreasing score
                            "content": line[:200]
                        })
                        break
            
            if len(results) >= top_k:
                break
        
        return results[:top_k]
