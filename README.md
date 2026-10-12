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

## 🔒 Concurrency & Watcher Safety (v0.2.0+)

From version `0.2.0`, `project-map-cli-rust` is fully multi-process and multi-agent safe:
- **Single-Writer Advisory Locking**: An advisory file lock (`<out_dir>/.lock`) prevents multiple simultaneous writers from conflicting during index generation and rotation.
- **Watcher Coalescing**: When multiple watchers (e.g. Claude Code and Antigravity agents running simultaneously on one workspace) receive file modification bursts, they acquire the lock in turn. Any watcher observing that an index was already rebuilt newer than the triggering event skips the rebuild and reloads immediately.
- **Pure & Resilient Readers**: Reader operations (`query`, `status`, `fetch`, etc.) are completely read-only and never write to the output directory. If an index is temporarily being updated, callers retry with exponential backoff (`load_with_retry`). If a reload encounters an error, the in-memory engine retains the previous valid index.
- **Atomic File Operations**: Index snapshots and `latest/` pointers are updated via atomic rename operations with `fsync`, ensuring readers never observe partially written files.

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
