#!/usr/bin/env python3
"""
Quick test to see if we can import and use the real retriever.
"""

import sys
from pathlib import Path

# Add project root to path
sys.path.insert(0, str(Path(__file__).parent.parent))

print("Testing imports...")

try:
    # Try to import core modules
    print("1. Importing config...")
    from core.config import Config, SearchMode
    print("   ✓ Config imported")
    
    print("2. Importing retriever...")
    from core.retriever import Retriever
    print("   ✓ Retriever imported")
    
    print("\n✅ Python API available! Can use RealCodeRetriever with Python API")
    print("\nTo use in benchmarks:")
    print("  retriever = RealCodeRetriever()")
    
except ImportError as e:
    print(f"\n❌ Import failed: {e}")
    print("\nThis means:")
    print("  1. Python bindings not available")
    print("  2. Will need to use CLI-based approach")
    print("  3. Or set up Python bindings for the Rust code")
