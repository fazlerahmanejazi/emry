# emry

A high-performance, offline-first CLI for semantic and structural code exploration. It combines precise static analysis with vector and lexical search to enable deep, context-aware reasoning over local git repositories.

## Core Architecture

Built as a modular Rust workspace optimized for latency and local execution.

- **`core`**: Domain logic and analysis.
  - **Stack Graphs**: Implements Stack Graphs to resolve symbols (definitions, references, imports) deterministically across files, enabling "jump to definition" and "find references" with compiler-grade accuracy.
  - **cAST (Context-Aware Splitting)**: Uses Tree-sitter to split code semantically (by function, class, block), preserving scope context for embeddings.
- **`index`**: Dual-head search engine.
  - **Vector**: `lance` for semantic embedding search (captures intent).
  - **Lexical**: `tantivy` for BM25 keyword search (captures exact matches).
- **`graph`**: `petgraph`-based knowledge graph. Maps entities (Files, Symbols, Chunks) and relationships (Calls, Imports, Defines) to enable multi-hop reasoning.
- **`store`**: Persistence layer on `sled`. Content-addressable storage for deduplicated file blobs and metadata.
- **`agent`**: LLM integration layer managing context windows and tool invocation (`fs`, `search`, `graph`).
- **`cli`**: Command-line orchestrator.

## Supported Languages

Powered by Tree-sitter and Stack Graphs for robust parsing and symbol resolution:

| Support Level | Languages | Features |
| :--- | :--- | :--- |
| **Full Graph** | Rust, Python, TypeScript, Java, Go | Call graphs, precise navigation, cross-file references |
| **Basic** | JavaScript, C, C++, C#, Ruby, PHP | Symbol extraction, text-based search |

## Prerequisites

- **Rust**: Stable toolchain (latest).
- **Git**: Required for branch detection.
- **LLM Provider**: OpenAI API key (or compatible endpoint) for embeddings and chat.

## Build & Install

```bash
cargo build --release
# binary located at target/release/emry
```

## Configuration

Create a `.emry.yml` in your project root or home directory.

```yaml
# Example .emry.yml
embedding:
  provider: "openai"
  model: "text-embedding-3-small"

llm:
  provider: "openai"
  model: "gpt-4-turbo"
  api_key: "sk-..." # Or via env var OPENAI_API_KEY
```

## Usage

### 1. Indexing
Builds the graph, generates embeddings, and populates the stores. Must be run before searching.

```bash
# Index the current repository
emry index 
```

### 2. Search
Perform hybrid retrieval against the index.

```bash
# Semantic + Keyword search
emry search "How is the RepoContext initialized?"
```

### 3. Graph Exploration
Query the static analysis graph directly.

```bash
# Find incoming calls to a symbol
emry graph --incoming "RepoContext"

# Find definitions in a file
emry graph --file "crates/context/src/context.rs"
```

### 4. Ask (Agent)
Interact with the codebase using the LLM agent. The agent autonomously uses search and graph tools to answer complex queries.

```bash
emry ask "Refactor the store trait to support async operations."
```

### 5. Status
Check the health and stats of the current index.

```bash
emry status
```

## Data Layout

Indexes are stored locally in `.codeindex/branches/<branch-name>/`.
- `store.db`: Sled database (metadata, chunks).
- `vector.lance`: LanceDB vector dataset.
- `lexical/`: Tantivy index segments.
- `graph.bin`: Serialized Petgraph structure.