![emry logo](emry.png)


Emry is a CLI tool designed for deep semantic and structural code exploration. It combines precise static analysis with vector and lexical search to enable deep, context-aware reasoning over local git repositories using natural language.

## Core Capabilities

*   **Interactive Code Querying**: Engage in natural language conversations with your codebase. The agent performs multi-hop reasoning, utilizing search, graph traversal, and filesystem operations to answer complex questions.
*   **Hybrid Search**: Combines semantic search (vector embeddings via LanceDB) with lexical search (BM25 ranking via Tantivy) for comprehensive retrieval.  Results are re-ranked based on code relationships.
*   **Semantic Chunking (cAST)**: Utilizes Tree-sitter AST parsing to split code by semantic boundaries (functions, classes, blocks), preserving context for accurate embeddings.
*   **Precise Code Navigation**: Employs Stack Graphs for accurate definition jumping, reference finding, and import tracing across files, handling complex scope resolution.
*   **Code Graph**: Builds a knowledge graph (using Petgraph) of codebase structure, representing files, symbols, and chunks as nodes, and their relationships (calls, imports, defines) as edges.
*   **Incremental Indexing**: Efficiently updates the index by tracking file changes via content hashing and re-indexing only modified files.
*   **Branch-Aware Storage**: Maintains separate indices for each Git branch, allowing seamless branch switching without index corruption.
*   **Offline-First**: All core indexing and querying operations run locally. External API calls are optional for embeddings and LLM inference.

## Architecture Overview

Emry is structured as a modular Rust workspace, with distinct crates handling specific functionalities.

### The Indexing Pipeline

This pipeline is responsible for processing the codebase and building the various data structures used for querying.

*   **`pipeline`**: Orchestrates the entire indexing workflow, managing concurrency and update processes.
*   **`core`**: Performs the foundational static analysis. It uses Tree-sitter for parsing and Stack Graphs for symbol resolution and semantic chunking (cAST).
*   **`graph`**: Constructs and manages the code property graph, capturing structural relationships.
*   **`index`**: Manages the LanceDB (vector embeddings) and Tantivy (lexical search) indices.
*   **`store`**: Provides the persistence layer, handling metadata (Sled), content-addressable storage for file blobs, and commit logs for incremental updates.

### The Querying & Agent Pipeline

This pipeline handles user interactions, interprets queries, and uses the indexed data to provide answers.

*   **`cli`**: The command-line interface entry point, responsible for parsing user commands (`index`, `search`, `ask`, `graph`, `status`) and dispatching them.
*   **`agent`**: The "cortex" of the system. It contains the LLM interaction logic, defines the interfaces (schemas) for tools, and orchestrates multi-step reasoning.
*   **`tools`**: Provides the concrete implementations of the functionalities used by the agent, such as `FsTool` for filesystem operations, `GraphTool` for graph traversal, and `Search` for querying the hybrid index.
*   **`context`**: Manages the shared application state and resource handles (e.g., database connections, configuration) that are passed across different components.
*   **`config`**: Handles the loading, parsing, and validation of the `.emry.yml` configuration file.

## Build & Usage

### Prerequisites

-   **Rust**: Stable toolchain (latest).
-   **Git**: Required for branch detection.
-   **LLM Provider**: OpenAI API key (or compatible endpoint) for embeddings and chat.

### Build & Install

```bash
cargo build --release
# binary located at target/release/emry
```

### Configuration

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

### Usage

#### 1. Indexing
Builds the graph, generates embeddings, and populates the stores. Must be run before searching.

```bash
emry index 
```

#### 2. Search
Perform hybrid retrieval against the index.

```bash
# Semantic + Keyword search
emry search "How is the RepoContext initialized?"
```

#### 3. Graph Exploration
Query the static analysis graph directly.

```bash
# Find incoming calls to a symbol
emry graph --incoming "RepoContext"

# Find definitions in a file
emry graph --file "crates/context/src/context.rs"
```

#### 4. Ask (Agent)
Interact with the codebase using the LLM agent. The agent autonomously uses search, graph and filesystem tools to answer complex queries.

```bash
emry ask "How does the indexing pipeline flow from CLI to storage?"
```

#### 5. Status
Check the health and stats of the current index.

```bash
emry status
```