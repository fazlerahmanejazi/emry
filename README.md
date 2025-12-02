![emry logo](emry.png)

<p align="center"><strong>Code intelligence for deep semantic and structural reasoning.</strong></p>

Emry is a fast, local-first CLI combining static analysis, code graphs, and hybrid search for natural-language code understanding.

## Features
- **Agent:** Multi-hop reasoning over structure and behavior.
- **Hybrid Search:** Semantic + lexical retrieval, reranked by graph relations.
- **Code Graph:** Tracks files, symbols, calls, and imports.
- **Smart Indexing:** Incremental, branch-aware, and semantically chunked (cAST).
- **Offline-First:** Local execution; external APIs optional.

## Install
```bash
cargo build --release
```

## Config
Configure via `.emry.yml` (or json/toml/env vars).

## Usage
- **Index:** `emry index` (Builds graph/embeddings)
- **Search:** `emry search "query"` (Hybrid retrieval)
- **Graph:** `emry graph --node "Symbol"` (Explore relations)
- **Ask:** `emry ask "question"` (LLM agent Q&A)
