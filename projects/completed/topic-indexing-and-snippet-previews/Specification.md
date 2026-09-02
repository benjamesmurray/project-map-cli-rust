# topic-indexing-and-snippet-previews - Specification

Kafka topic, SQL table, and env var indexing with contextual snippet previews in `project-map-cli-rust`.

## Core Features & Scope

### In-Scope
1. **Architecture & Contract Entity Extraction**:
   - Introduce `NodeType` variants: `KafkaTopic`, `DatabaseTable`, and `ConfigEnvVar` (with backwards compatibility for existing `File` and `Symbol`).
   - Extract Kafka topics from:
     - YAML / Docker Compose files (e.g. `KAFKA_TOPIC`, topic lists, env bindings).
     - Kotlin and Python source code (string literals matching topic patterns such as `*.v[0-9]+`, or variables assigned in producer/consumer configs).
   - Extract Database tables from:
     - SQL DDL scripts (`CREATE TABLE`, `ALTER TABLE`, migration files).
     - SQL queries in source code where table definitions or targets are explicitly referenced.
   - Extract Environment variables (`ConfigEnvVar`) from:
     - `.env*` files.
     - Docker Compose / Kubernetes YAML `environment:` definitions.
     - `System.getenv(...)` (Kotlin/Java) and `os.environ` / `os.getenv` (Python).
   - Relationship / Role classification:
     - Classify Kafka topic usages as `Producer`, `Consumer`, or `Configuration`.
2. **Contextual Snippet Previews**:
   - Add CLI support for `--preview [lines]` / `-C <N>` (defaulting to 3 lines) to `map find` and `map search`.
   - Include contextual snippets in MCP tool calls (`map:search`, `map:query`, `map:blast_radius`, `map:fetch_symbol`).
   - Snippet includes: docstring/comments if immediately preceding, declaration/signature line, and context lines of the body with 1-based line numbers.
   - Exact matching and fuzzy highlight excerpts for query terms.
3. **Incremental Watcher & Query Engine Updates**:
   - Ensure the continuous file watcher (`-w`) indexes `.env*`, `.yaml`, `.yml`, and `.sql` changes.
   - Query engine enhancements to support filtering or searching by entity kind (e.g. topic, table, env) and returning structured classification results.

### Out-of-Scope
- Complex AST dataflow analysis or interprocedural taint analysis for dynamic topic construction at runtime.
- Full SQL semantic query parsing beyond DDL definitions and standard table references.
- External schema registry (Confluent Schema Registry / Avro / Protobuf wire format) network fetching.

## Technical Constraints
- **Language/Runtime**: Rust 2021 edition, maintaining compatibility with `rust-mcp-sdk 0.9.0` and `tree-sitter`.
- **Deserialization & Backward Compatibility Guard**:
  - `NodeType` and `EdgeType` must use `#[serde(other)] Unknown` fallback variants to ensure existing or future `.project-map.json` caches deserialize cleanly.
  - If a schema or deserialization mismatch occurs when loading an existing `.project-map.json`, the engine must gracefully log a warning and trigger a clean re-index rather than crashing or aborting.
- **Performance & Zero Latency**:
  - `SnippetPreview` must NEVER be serialized into the `.project-map.json` index on disk. The index only retains line numbers, byte offsets, and entity metadata to preserve compact index size (~4MB vs 100+MB).
  - Snippets are extracted strictly on-demand at query time from disk for the top N matches, leveraging an in-memory file line cache for repeated hits.
- **Parsing Boundaries**:
  - Reserve Tree-sitter for AST-driven source code (Kotlin, Python, Rust, SQL DDL).
  - Use `serde_yaml` for YAML / Docker Compose parsing and lightweight line-by-line scanning for `.env*` files to avoid verbose and brittle tree-sitter YAML CST traversals.
- **MCP Conformance**:
  - Tool responses in `CallToolResult.content` format code snippets using standard Markdown code blocks with line numbering.
  - Structured tool payloads retain backwards-compatible fields (`name`, `kind`, `path`, `line`, `role`, `preview`).

## User Stories
- **US-1**: As an AI coding agent exploring an event-driven microservice repository, I want to query a Kafka topic (e.g. `map find wde.labels.phase.v1`) and receive a categorized list of its producers, consumers, definitions, and references, so that I can trace event flow without performing unbounded grep searches.
- **US-2**: As a developer maintaining database migrations, I want `map find <table_name>` to point directly to table definitions in DDL migrations and references in service code, so that I can evaluate database schema dependencies quickly.
- **US-3**: As a developer or agent inspecting search results, I want `map find <symbol> --preview 3` or `map:search` to return surrounding context lines and docstrings directly in the response, so that I do not need a subsequent `view_file` call to inspect function signatures and comments.
- **US-4**: As a developer editing `.env` or Compose files, I want the background watcher to immediately re-index newly added environment variables and topic configurations.
- **US-5**: As a user running an older `.project-map.json` cache, I want upgrading the CLI to succeed without deserialization panics, falling back to a clean re-index if needed.

## Overview & Architecture
The indexing pipeline follows a 3-stage process:
1. **Parser & Entity Extraction Stage**:
   - `CodeParser` receives source files, SQL files, YAML files, and `.env` files.
   - For `.env*`: Fast line-by-line scanner splitting on `=` to capture environment variable keys.
   - For YAML / Docker Compose: `serde_yaml` parser extracting `environment:` variables, `KAFKA_TOPIC_*`, `input-topics`, `output-topic`, etc.
   - For SQL files: Tree-sitter parses `CREATE TABLE` and `ALTER TABLE` DDL statements into `NodeType::DatabaseTable`.
   - For programming languages (Kotlin, Python, Rust, TS): Tree-sitter AST queries identify standard symbols, plus dedicated visitors for:
     - String literals matching Kafka topic naming patterns (`*.v[0-9]+` or topic config keys).
     - Concrete heuristic classification:
       - **Producer**: CLI flags/keys containing `output-topic`, `output_topic`, `sink`, `producer`, or AST calls to `.send(...)`, `.produce(...)`, `.to(...)`.
       - **Consumer**: CLI flags/keys containing `input-topics`, `input_topic`, `source`, `consumer`, or AST calls to `.subscribe(...)`, `.from(...)`, `.consume(...)`, `GlobalKTable(...)`.
       - **Configuration**: Declarations in YAML/Compose/config files.
       - **Reference**: Fallback when no directional indicators are present.
     - Calls to `System.getenv(...)` and `os.environ` / `os.getenv(...)`.
2. **Graph & Storage Stage**:
   - `ProjectGraph` in `src/core/graph.rs` stores nodes with resilient `NodeType` and `EdgeType` enums containing `#[serde(other)] Unknown`.
   - No snippet text is stored in `ProjectGraph` or `.project-map.json`.
3. **Query & Presentation Stage**:
   - `QueryEngine` in `src/core/query_engine.rs` resolves queries against symbol names, topics, tables, and env vars.
   - `SnippetExtractor` slices source files on-demand from disk for the top matches, with line numbering and markdown formatting.
   - CLI commands (`src/cli/commands.rs`) and MCP server handlers (`src/mcp/server.rs`) present Markdown code blocks and structured JSON responses.

## Technical Stack
- **Core**: Rust (1.80+), `tokio`, `serde`, `serde_json`, `petgraph`.
- **Parsing**: `tree-sitter`, `tree-sitter-sequel`, `tree-sitter-python`, `tree-sitter-kotlin-ng`, `tree-sitter-rust`, `tree-sitter-typescript`, `serde_yaml`.
- **MCP**: `rust-mcp-sdk 0.9.0`.
- **CLI**: `clap` with derive features.
- **File Watching**: `notify`.

## Components and Interfaces

### 1. `src/core/graph.rs`
- Resilient `NodeType` and `EdgeType`:
  ```rust
  #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
  pub enum NodeType {
      File,
      Symbol,
      KafkaTopic,
      DatabaseTable,
      ConfigEnvVar,
      #[serde(other)]
      Unknown,
  }

  #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
  pub enum EdgeType {
      Contains,
      Imports,
      Calls,
      Produces,
      Consumes,
      Configures,
      References,
      #[serde(other)]
      Unknown,
  }
  ```
- Deserialization mismatch fallback in `ProjectGraph::load(path)` to automatically trigger re-indexing on deserialization failure.

### 2. `src/core/parser.rs`
- Fast `.env` scanner (`parse_env_file`).
- Structured `serde_yaml` extractor (`parse_yaml_configs`).
- SQL DDL table extractor using Tree-sitter sequel (`parse_sql_file`).
- Code scanners with directional topic heuristics:
  - Matcher for Kafka calls (`send`, `to`, `produce` vs `subscribe`, `from`, `consume`, `GlobalKTable`).
  - Matcher for environment variable access (`System.getenv`, `os.environ`).

### 3. `src/core/query_engine.rs` & `src/core/toon.rs`
- `SnippetExtractor` (on-demand, not stored in graph):
  - Slices target files around symbol line numbers with 1-based numbering.
  - Formats as markdown code blocks:
    ````markdown
    ```kotlin
    141:   // Output changelog topic
    142:   val outputTopic = "wde.labels.phase.v1"
    143:   builder.stream(outputTopic)
    ```
    ````
  - Caches file lines per file during a single query session.

### 4. `src/cli/commands.rs` & `src/mcp/server.rs`
- Add `--preview [lines]` (alias `-C <N>`, default 3) to CLI `find` and `search`.
- Enrich MCP `map:query` and `map:search` JSON objects with `role` and `preview`, and format markdown text output with line numbers.

## Data Models

```rust
// Only generated on-demand at query time
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnippetPreview {
    pub target_line: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub lines: Vec<(usize, String)>,
    pub formatted: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityMatch {
    pub name: String,
    pub kind: String,
    pub node_type: NodeType,
    pub path: String,
    pub line: usize,
    pub role: Option<String>, // "Producer", "Consumer", "Configuration", "Reference"
    pub preview: Option<SnippetPreview>,
}
```