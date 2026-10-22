# Project Map CLI (Rust)

A high-performance, idiomatic Rust reimplementation of `project-map-cli`. This tool provides AI agents and developers with a contextually efficient architectural map of a codebase, enabling deep navigation and dependency analysis without massive token overhead.

## 🚀 Features
- **Agent-Native TOON Output:** Standardized "Token-Oriented Object Notation" presentation layer designed specifically for LLM context efficiency and readability.
- **Multi-Language AST Parsing:** Powered by Tree-sitter for high-fidelity symbol extraction.
  - Supported: **Rust, Python, TypeScript/TSX, Kotlin, SQL, Vue 3, Lua, PHP (Symfony), Terraform, GDScript**.
  - **Modern TypeScript:** Robust support for Arrow Functions, Enums, Interfaces, and Re-exports.
  - **PHP & Symfony:** Deep support for PSR-4 namespaces, Symfony Routing Attributes, and YAML-based service configuration.
  - **Terraform:** Extraction of Resources, Modules, Variables, and Data sources, with module-level dependency mapping.
  - **GDScript (Godot):** Support for `class_name`, `signal`, and `function` definitions. Cross-file mapping handles Godot's `res://` path prefix for `preload`, `load`, and `extends`.
  - **Semantic Docstring Search:** Comments and JSDoc/KDoc are indexed and associated with their respective symbols, allowing you to search by architectural intent.
- **Architectural Discovery:**
  - `find`: Fast substring search for symbols across the monorepo.
  - `context`: Dense architectural overview of any source file.
  - `blast`: Inbound dependency analysis (who depends on this?).
  - `impact`: Outbound dependency analysis (what does this depend on?).
  - `fetch`: Precise extraction of raw source code using byte-range hydration.
- **Smart Versioning & Reliability:**
  - **Cold Start Auto-Scaffolding:** Automatically creates an `ARCHITECTURE.md` template in empty projects during `pm_init` to provide an immediate landing pad for AI agents.
  - Automatic `.gitignore` respect for clean indexing.
  - Explicit self-exclusion of the `.project-map` directory to avoid metadata noise.
  - **Custom Index Paths:** Use `--out` and `--index` to manage maps for external projects without cluttering source directories.
  - Rotating backups: Automatically maintains the **5 most recent builds** to save space.
  - Consistent `latest/` symlink for stable integration.
- **MCP Server & Auto-Discovery:** Built-in Model Context Protocol server exposing readable resources for startup auto-discovery and standard tools for codebase interaction. Powered by `rust-mcp-sdk`.
  - **MCP Resources:**
    - `project-map://status`: Instant JSON workspace status, symbol count, file count, and last-indexed timestamp.
    - `project-map://tree`: ASCII directory tree visualization and architectural pulse.
  - **MCP Tools:**
    - `status`: Workspace context, component tree, and active specification feature discovery (filters only valid features containing `Specification.md`, `Tasks.json`, or `.deliver_meta.json`).
    - `query`: Search for symbols or file outlines.
    - `blast_radius`: Identifies all components and files that depend on or import a specific symbol.
    - `plan`: Analyze the architectural impact (fan-out) of a symbol.
    - `search`: Search for logic using natural language keywords (e.g., 'auth', 'database').
    - `fetch_symbol`: Extract raw source code for a specific class or function.
    - `init`: Refresh the map index after significant code changes.
    - Documentation tools (v0.3.0+, TOON output capped at 4,000 characters with a truncation marker): `doc_search`, `doc_outline`, `doc_section`, `doc_links`, `doc_terms`, `api_surface`. See [Documentation Indexing](#-documentation-indexing-v030).
  - Every tool description says when to use it instead of grep or a text search.
- **Specification-Driven Project Pulse:**
  - Accurately reports active features in `status` (CLI & MCP) by verifying the presence of specification artifacts (`Specification.md`, `Tasks.json`, or `.deliver_meta.json`), preventing empty, leftover, or category directories in `projects/active/` from cluttering reports.
- **Background File-System Watcher:**
  - `project-map watch`: Incremental auto-indexing background daemon using `notify`. Automatically re-indexes changed files with debouncing while ignoring `.project-map`, `.git`, and `target` directories.
  - `project-map mcp --watch`: Start the MCP server with automatic background file watching enabled.

## 🛠 Installation

```bash
cargo install project-map-cli-rust
```

## 📖 Usage

### 1. Build the Map
Index your project and create a versioned snapshot.
```bash
# Standard usage
project-map build --root .

# Build for an external project into a specific directory
project-map build --root /path/to/other/project --out ./external-map
```

### 2. Find a Symbol
```bash
# Search using default .project-map index
project-map find --query "MyService"

# Search using a specific index directory
project-map find --query "MyService" --index ./external-map
```

### 3. Analyze Blast Radius
See everything that might break if you change a specific symbol.
```bash
project-map blast --path "src/core/utils.rs" --symbol "my_helper"
```

### 4. Fetch Raw Logic
Extract just the code you need.
```bash
project-map fetch --path "src/main.rs" --symbol "main"
```

### 5. Start MCP Server & Background Watching
Connect to Claude, Gemini, opencode, or other agents.
```bash
# Standard MCP server
project-map mcp

# Start MCP server with background file watching enabled
project-map mcp --watch

# Standalone background file watcher
project-map watch --root .
```

### 6. Workspace Status & Diagnostics
View comprehensive index health, symbol metrics, build timestamps, and active specification features:
```bash
project-map status
```
`status` automatically detects and warns if:
- The index contains 0 files or only output directory files (degenerate index).
- The index root differs from the current workspace root.
- The index was generated by a legacy version without metadata.

### 7. Query Documentation (v0.3.0+)
```bash
project-map doc search "audit log" --limit 10      # ranked sections: path, anchor, heading, snippet
project-map doc outline docs/adr/004.md            # heading tree with anchors, plus frontmatter
project-map doc section docs/adr/004.md decision   # one section's text (--max-chars, default 4000)
project-map doc links src/audit/log.rs --symbol append   # docs that mention the symbol
project-map doc links docs/adr/004.md              # files and symbols the document references
project-map doc terms docs --limit 30              # recurring domain terms under a path prefix
project-map api openapi.yaml                       # OpenAPI operations and schemas (path optional)
```
All of these take `--index <dir>` (default `.project-map`) and print exactly what the matching MCP tool returns.

## 📚 Documentation Indexing (v0.3.0+)

The index now holds what the documentation says as well as the code structure. It is deterministic: no LLM, no embeddings.

- **Markdown sections:** every heading becomes a section with a GitHub-style anchor (`foo`, `foo-1`, `foo-2` for duplicates) and a byte range. Section text is **not** stored in the index; `doc_section` reads the file on demand.
- **Frontmatter:** `id`, `title`, `status`, `date`, `domain`, `tags` and `supersedes` (ADRs, specs) are recorded per file; `doc_search` matches `id`, `title` and `tags`.
- **Doc → code links:** a section that mentions code gets a reference to it, attributed to the deepest section. Matching is exact, in this order:
  1. Markdown links to indexed relative paths.
  2. Path forms in code spans and fenced code (`src/core/graph.rs`).
  3. Path-qualified symbols (`audit/log.rs::append`, `src/core/graph.rs:191`).
  4. Type-qualified symbols (`Graph::load`, `Graph.load`), only when exactly one match exists.
  5. Bare names, only when exactly one symbol has the name, it has at least 4 characters, and it is not a generic word (`new`, `main`, `run`, ...).

  Anything else is counted as unresolved or ambiguous (unmatched bare names in fenced code are not counted) and shown by `status`. Existing code tools (`query`, `search`, `plan`, `blast_radius`, `fetch_symbol`) stay code-only; use `doc_links` for the docs that mention a symbol.
- **OpenAPI:** files named `openapi.*` or `swagger.*`, or YAML/JSON files up to 1 MB whose first 1 KB has a top-level `openapi` or `swagger` key, produce operation (`METHOD /path`, `operationId`, summary) and schema entries. `api_surface` lists them.
- **Terms:** capitalised runs of 2-4 words and bold spans that occur at least twice in a directory, top 50 per directory, each with up to 5 sections. A cheap hint for actors and vocabulary, not an extraction.
- **Schema v3 and stale indexes:** the index `schema_version` is now 3. An older index still serves the code tools, is **never rewritten by a query**, and `status` and the doc tools say to run `project-map refresh` (or the MCP `init` tool). A build refuses to overwrite an index with a *newer* schema than it knows.

## 🔒 Concurrency & Watcher Safety (v0.2.0+)

From version `0.2.0`, `project-map-cli-rust` is fully multi-process and multi-agent safe:
- **Single-Writer Advisory Locking**: An advisory file lock (`<out_dir>/.lock`) prevents multiple simultaneous writers from conflicting during index generation and rotation.
- **Watcher Coalescing**: When multiple watchers (e.g. Claude Code and Antigravity agents running simultaneously on one workspace) receive file modification bursts, they acquire the lock in turn. Any watcher observing that an index was already rebuilt newer than the triggering event skips the rebuild and reloads immediately.
- **Pure & Resilient Readers**: Reader operations (`query`, `status`, `fetch`, etc.) are completely read-only and never write to the output directory. If an index is temporarily being updated, callers retry with exponential backoff (`load_with_retry`). If a reload encounters an error, the in-memory engine retains the previous valid index.
- **Atomic File Operations**: Index snapshots and `latest/` pointers are updated via atomic rename operations with `fsync`, ensuring readers never observe partially written files.

## 🔄 Upgrading to 0.3.0

```bash
# 1. Update the binary
cargo install project-map-cli-rust --force

# 2. Restart EVERY running `project-map-cli-rust mcp` process (all editors, agents and
#    terminals, in every repository) BEFORE the first 0.3 build.

# 3. Rebuild each workspace index once
project-map refresh
```

**Why step 2 matters:** a running 0.2.x watcher does a full rebuild on any file change and writes a schema-2 index over a schema-3 one, silently dropping the documentation index. 0.3.0 cannot protect an index from 0.2.x binaries that are still running. Until step 3, a 0.2.x index keeps working for code tools and is reported as stale by the doc tools.

## 🔄 Upgrading from 0.1.x

To upgrade an existing installation:
```bash
# 1. Update the binary
cargo install project-map-cli-rust --force

# 2. Restart all running MCP clients / coding assistant sessions
# (e.g. Claude Code, Antigravity, OpenCode) so the new binary is loaded into memory

# 3. Refresh or rebuild the workspace index once
project-map refresh
```

*Note on backward compatibility:* Indexes built with `0.1.18` (which used `./` path prefixes and had no metadata block) load automatically without errors in `0.2.0` and have their paths normalized in memory.

## 📂 Storage Structure

The tool maintains state in the `.project-map/` directory (or your custom `--out` directory):
- `latest/`: Symlink (or atomic pointer) to the most recent successful build.
- `backups/`: Historical snapshots (limited to 5) with millisecond-and-PID resolution (`YYYYMMDD_HHMMSS_mmm_<pid>`).
- `.lock`: Advisory single-writer lock file (never deleted).

## License
MIT
