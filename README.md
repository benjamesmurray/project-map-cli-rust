# Project Map CLI (Rust)

A high-performance, idiomatic Rust reimplementation of `project-map-cli`. This tool provides AI agents and developers with a contextually efficient architectural map of a codebase, enabling deep navigation and dependency analysis without massive token overhead.

## 🚀 Features
- **Agent-Native TOON Output:** Standardized "Token-Oriented Object Notation" presentation layer designed specifically for LLM context efficiency and readability.
- **Multi-Language AST Parsing:** Powered by Tree-sitter for high-fidelity symbol extraction.
  - Supported: **Rust, Python, TypeScript/TSX, Kotlin, SQL, Vue 3**.
- **Architectural Discovery:**
  - `find`: Fast substring search for symbols across the monorepo.
  - `context`: Dense architectural overview of any source file.
  - `blast`: Inbound dependency analysis (who depends on this?).
  - `impact`: Outbound dependency analysis (what does this depend on?).
  - `fetch`: Precise extraction of raw source code using byte-range hydration.
- **Smart Versioning & Reliability:**
  - Automatic `.gitignore` respect for clean indexing.
  - Explicit self-exclusion of the `.project-map` directory to avoid metadata noise.
  - Rotating backups: Automatically maintains the **5 most recent builds** to save space.
  - Consistent `latest/` symlink for stable integration.
- **MCP Server:** Built-in Model Context Protocol server exposing `pm_status`, `pm_query`, `pm_check_blast_radius`, and `pm_plan` tools. Powered by `rust-mcp-sdk` for fully type-safe compliance with the `2024-11-05` protocol.

## 🛠 Installation

```bash
cargo install project-map-cli-rust
```

## 📖 Usage

### 1. Build the Map
Index your project and create a versioned snapshot.
```bash
project-map build --root .
```

### 2. Find a Symbol
```bash
project-map find --query "MyService"
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

### 5. Start MCP Server
Connect to Claude, Gemini, or other agents.
```bash
project-map mcp
```

## 📂 Storage Structure
The tool maintains state in the `.project-map/` directory:
- `latest/`: Symlink to the most recent successful build.
- `backups/`: Historical snapshots (limited to 5) of the project's architecture.

## License
MIT
