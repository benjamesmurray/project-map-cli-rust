# Project Map CLI (Rust)

A high-performance, idiomatic Rust reimplementation of `project-map-cli`. This tool provides AI agents and developers with a contextually efficient architectural map of a codebase, enabling deep navigation and dependency analysis without massive token overhead.

## 🚀 Features
- **Agent-Native TOON Output:** Standardized "Token-Oriented Object Notation" presentation layer designed specifically for LLM context efficiency and readability.
- **Multi-Language AST Parsing:** Powered by Tree-sitter for high-fidelity symbol extraction.
  - Supported: **Rust, Python, TypeScript/TSX, JavaScript (v0.4.0+), Kotlin, SQL, Vue 3, Lua, PHP (Symfony), Terraform, GDScript**.
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
    - `blast_radius`: Identifies all files that depend on or import a specific symbol, each at its shortest depth, with direct and transitive counts. It counts dependents (importers, stream and config links), not callers: the index has no call edges.
    - `plan`: Analyze the architectural impact (fan-out) of a symbol.
    - `search`: Search for logic using natural language keywords (e.g., 'auth', 'database').
    - `fetch_symbol`: Extract raw source code for a specific class or function.
    - `init`: Refresh the map index after significant code changes.
    - `modules` (v0.4.0+): files, code files, symbols, doc files and doc sections per directory, to see which areas are largest.
    - `imports` (v0.4.0+): which files use an external package, with the import line (grep cannot tell an import from a mention).
    - Every tool takes `format: "json"` for programs (v0.4.0+). See [JSON Output](#-json-output-v040).
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
See everything that might break if you change a specific symbol: each dependent at its shortest depth (1 is a direct importer), the target's own file not included. `direct` counts dependents, not callers.
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

### 7. Modules, Imports and JSON (v0.4.0+)
```bash
project-map modules src --max-depth 2                # sizes per directory, recursive counts
project-map imports stripe --limit 20                # files that import the package, with the line
project-map imports "@sentry/*" --path-prefix apps/  # prefix match, restricted to a path prefix
project-map blast --path src/core/utils.rs --symbol my_helper --format json --limit 100
project-map status --max-depth 1 --format json
```
`--format toon|json` is accepted by `find`, `context`, `impact`, `status`, `fetch`, `blast`, `search`, `doc`, `api`, `modules` and `imports`. `--limit` applies to `find`, `search`, `blast`, `impact` and `imports`; `--max-depth` to `status`, `modules` and `doc outline`. JSON goes to stdout only. A JSON error prints the error envelope to stdout and exits 1 (a TOON error still prints and exits 0). `build`, `refresh`, `watch` and `mcp` reject `--format`.

### 8. Query Documentation (v0.3.0+)
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

## 🤖 Using with Claude Code (v0.4.0+)

**Claude Code defers MCP tools by default.** With tool search on (the default), every MCP tool is deferred: at session start Claude sees only the tool *names* and the server's *instructions*, and loads a tool's schema when it searches for it. So an agent that does not think to search will never use the `map` tools.

What this server does about it: it sends MCP `instructions` (about 1,300 characters, well inside Claude Code's 4,096-character limit). They say what the index covers, to prefer the map tools over grep for symbols, dependents, imports and docs, and name every tool in a few words, so a search for any of those words finds the tool. The instructions reach the model even while the tools are deferred.

Settings that load the tools at session start (nothing here changes them for you):

| Who | Setting | Effect |
|:--|:--|:--|
| You, per server | `"alwaysLoad": true` in the server's entry in `~/.claude.json` or `.mcp.json` | Loads this server's tools upfront (the best fit: one server, a small tool list). |
| You, global | `ENABLE_TOOL_SEARCH=false`, or `auto` / `auto:N` | `false` loads every MCP tool of every server upfront; `auto:N` loads them while their definitions stay under N % of the context window. |
| Server author, per tool | `"_meta": {"anthropic/alwaysLoad": true}` on a tool | Loads that tool upfront. This server does **not** set it, because it costs context in every session of every client; it is your call. |

Example server entry:
```json
{ "mcpServers": { "map": { "type": "stdio", "command": "project-map-cli-rust", "args": ["mcp", "-w"], "alwaysLoad": true } } }
```
After changing it, check that `mcp__map__status` is callable without a tool-search step. Claude Code's documentation describes these settings; this release read them and did not trial `alwaysLoad` in a fresh session.

## 🧾 JSON Output (v0.4.0+)

Every MCP tool takes `format` (`"toon"`, the default, or `"json"`), and every CLI subcommand above takes `--format`. TOON keeps its 0.3.0 content (see *Compatibility* below); JSON is for programs and costs more tokens. With `json`, the MCP result's text content holds the document and `structured_content` holds the same object. Tools declare no `outputSchema`, because structured content exists only with `json`. JSON is never cut by characters: lists stop at the tool's item limit (`limit`; default 50 for `search` and `query`, 200 for `blast_radius` and `plan`), and `truncated` and `total` say what was left out.

### Envelope

```json
{
  "tool": "blast_radius",
  "contract": 1,
  "index": { "state": "ok", "schema_version": 4, "commit": "<40-hex or null>", "dirty": false, "stale": false },
  "truncated": false,
  "total": 3,
  "summary": { "direct": 2, "transitive": 1 },
  "results": [ { "symbol": "…", "path": "…", "depth": 1 } ]
}
```

- `index.state` is `ok`, `stale` or `missing`, so a program can tell a missing index from an empty result. When `missing`, the other `index` fields are `null`.
- `total` is the number of matches before any limit; summary counts always cover every match.
- A field that does not apply is omitted, never `null`, except the Git fields in `status`.
- Lines are 1-based; `end_line` is the last line of the node's byte range. File nodes have no `line`, `end_line` or `target_line`.
- `symbol` is qualified when it lies inside another symbol (`Graph.load`). Qualified names are not unique (two `impl From<…> for E` blocks both give `E.from`), so cite `path` and `line` too.
- Key order in the document is alphabetical, and the same tree gives the same bytes.

### Errors

An error has `"error": {"code", "message"}` and no `summary` or `results`; MCP errors also set `is_error` (in TOON too).

| Code | Meaning |
|:--|:--|
| `index_missing` | No index at the path. Every tool but `status` returns it; `status` reports `index.state: "missing"` and a `next_step`. |
| `index_stale` | The index predates what the tool needs: `imports` needs schema 4, the doc tools schema 3. Run `project-map refresh` (or the `init` tool). |
| `not_found` | The path, symbol or section does not exist in the index. |
| `invalid_argument` | A bad argument (for example an unknown `format` or an empty `package`). |
| `io_error` | A file the index points at could not be read. |

### Fields

| Tool | Item fields | Summary fields |
|:--|:--|:--|
| `status` | none (`results` is `[]`) | `next_step` (index missing only), `tool_version`, `schema_version`, `root`, `built_at`, `commit`, `dirty`, `branch`, `head_now`, `dirty_now`, `at_head`, `git_error`, `stale`, `files`, `symbols`, `edges`, `doc_files`, `doc_sections`, `doc_references`, `api_operations`, `tree`, `active_features` |
| `search`, `query` (query form) | `symbol`, `kind`, `node_type`, `path`, `line`, `end_line`, `role`, `preview` | none |
| `query` (path form) | `symbol`, `kind`, `line`, `end_line`, `role`; `path` on import entries | `path` |
| `query` (filename form) | `path` | none |
| `fetch_symbol` | none | `path`, `symbol`, `line`, `end_line`, `source` |
| `blast_radius` | `symbol`, `kind`, `node_type`, `path`, `line`, `end_line`, `depth` | `target_path`, `target_symbol`, `target_line`, `matched`, `via`, `direct`, `transitive`, `total`, `files_affected`, `languages` |
| `plan` | `symbol`, `kind`, `node_type`, `path`, `line`, `depth` | `requested`, `target_symbol`, `target_path`, `target_line` |
| `doc_search` | `path`, `anchor`, `heading`, `level`, `line`, `score`, `snippet` | `query` |
| `doc_outline` | `anchor`, `heading`, `level`, `depth`, `line`, `end_line` | `path`, `frontmatter` |
| `doc_section` | none | `path`, `anchor`, `heading`, `line`, `end_line`, `text` |
| `doc_links` | `direction` (`to_docs`, `to_code`), `path`, `anchor`, `heading`, `target_path`, `target_symbol`, `form` | `path`, `symbol`, `total` |
| `doc_terms` | `term`, `count`, `sections` | `path_prefix` |
| `api_surface` | `path`, `item`, `method`, `route`, `operation_id`, `name`, `summary`, `line` | `total` |
| `modules` | `path`, `depth`, `files`, `code_files`, `symbols`, `doc_files`, `doc_sections` | `path_prefix`, `max_depth` |
| `imports` | `path`, `line`, `text`, `module`, `package`, `language`, `resolved` | `query`, `files`, `imports` |
| `init` | none | `status` (`refreshed` or `failed`), `message`, `commit`, `dirty` |

### Contract versioning

`contract` is the JSON contract version, starting at 1, and is independent of the index `schema_version`. Adding, removing or renaming a field, changing a value's type or changing an error code is a **breaking** change: before 1.0 it needs a minor version bump and a **Breaking** entry in the CHANGELOG, and `contract` goes up. Thirty-nine golden envelopes in `tests/fixtures/json_contract/` pin the contract; its README explains how to regenerate them.

## 🧭 Status, Git State and Depth Limits (v0.4.0+)

- **Indexed commit.** A build records the Git commit of the root, whether the tree was dirty (untracked files count, ignored files and `.project-map` do not) and the branch (`null` when detached). `status` also reads `HEAD` and the dirty flag now and reports `head_now`, `dirty_now` and `at_head` (the indexed commit equals `HEAD`, and neither is dirty). TOON prints `Commit: 1a2b3c4 (clean) | at_head: yes`.
- Git runs as a subprocess with `GIT_OPTIONAL_LOCKS=0`, so it never takes `.git/index.lock`. At build time each call has a 5 s timeout; at query time all calls share one 1.5 s budget. Outside a repository, with Git missing or timed out, the fields are `null` and everything else works. If Git ran but failed (for example "dubious ownership" on a checkout owned by another user), a warning goes to stderr and `status` shows the first line as `git_error`.
- **`max_depth`.** `status(max_depth)` renders the tree to that depth (default 3). `doc_outline(path, max_depth)` keeps headings by nesting depth, where an h3 directly under an h1 is depth 2. Both report that they were cut: `truncated` in JSON, a `[tree cut at depth N]` or `[outline cut at depth N]` note in TOON. Without `max_depth`, TOON is the 0.3.0 text.

## 📦 Modules and Imports (v0.4.0+)

**`modules(path_prefix?, max_depth=2)`** lists each directory under the prefix down to `max_depth` levels with `files`, `code_files` (a recognised language), `symbols` (code symbols; doc and API nodes excluded), `doc_files` and `doc_sections`. Counts are recursive, including directories below `max_depth`. Directories are sorted by path; files at the repository root count under `.`.

**`imports(package, path_prefix?, limit=50)`** returns every import of an external package: path, line, the trimmed import line (at most 200 characters), the package, `resolved` (linked to an indexed file or symbol) and the language. A package matches exactly; a query ending in `/*` or `.*` matches by prefix (`@sentry/*` matches `@sentry/node` and `@sentry/react`). A Python package is named by its first segment, so `imports google` finds `google.cloud` imports and `imports "google.*"` does not; the prefix form is for scoped JavaScript packages (`@sentry/*`). `files` and `imports` count every match, not only the `limit` listed. Results are ordered by path, line, module (`import a, b` is two records on one line). A schema-3 index gives `index_stale`.

Package names:

| Language | Package name |
|:--|:--|
| Python | first dotted segment (`sentry_sdk.integrations` is `sentry_sdk`) |
| JavaScript, TypeScript | scoped keeps two segments (`@sentry/node/x` is `@sentry/node`), unscoped keeps one (`stripe/lib` is `stripe`); relative and absolute paths have none; URL imports have none |
| Rust | first path segment of a `use` (`serde_json::Value` is `serde_json`); `crate`, `self`, `super` and the crate's own name have none; `std`, `core` and `alloc` are packages like any other |
| PHP | first namespace segment |
| Kotlin | the qualified name minus its last segment |
| Lua | the `require` argument up to the first `.` |
| HCL, GDScript | none (they import files) |

### JavaScript and Rust coverage

- `.js`, `.jsx`, `.mjs` and `.cjs` files are indexed with the TSX grammar (symbols and imports, as TypeScript). `require("x")` and `import("x")` with a string literal are captured in JavaScript and TypeScript.
- Built and minified JavaScript is **skipped** (no symbols, no imports): a file named `*.min.*`, larger than 256 KB, or with a line over 1,000 characters in its first 32 KB. TypeScript files are never skipped.
- Rust `use` declarations and `extern crate` are captured, one record per statement. `crate::`, `self::`, `super::` and own-crate-name paths resolve to `Imports` edges (to the symbol, else to the file; each leaf of a grouped `use`), so `blast_radius` works on Rust. The crate root is the nearest ancestor `Cargo.toml`.

## 💥 Blast Radius Meaning (v0.4.0+)

`blast_radius` walks incoming edges breadth-first and lists each dependent once, at its shortest distance: `depth` 1 is a direct importer (or a stream or config link), 2 and above are transitive. The graph has no call edges, so **`direct` counts dependents, not callers**. The target's own file and enclosing symbols are never listed. `files_affected` counts distinct dependent files, and `languages` lists their languages, sorted.

- `matched` is `symbol`, or `file` when the symbol was not found in the path and the file's radius is used (the walk then starts from the file and every symbol in it). Neither found is `not_found`.
- A target nested in another symbol (a method) is counted through its enclosing symbols and the same-named symbols of the file (the struct an `impl` block belongs to), because no import names a method. The summary names the outermost one as `via`, TOON prints `Counted via enclosing symbol X: no import names a method.`, and the counts include importers that never use the method. A type, a free function or a file target has no `via`.
- With a TOON `limit` that cuts the list, `[limit N: M matched]` is appended so the total is not lost.

## 🖥 Server Mode (v0.4.0+)

- `mcp` without `-w` starts no watcher and never builds on its own. With `-w` it builds and watches.
- `status` answers at once when the index is missing, stale or at another schema, and says what to do (`refresh` or the `init` tool).
- With a missing index, every other tool returns `index_missing` (with `is_error`); it does not crash or hang. With a stale (schema-3) index the code tools keep working, `imports` returns `index_stale`, and the doc tools work from schema 3.
- stdout carries only JSON-RPC frames. stderr carries no ANSI escape codes unless it is a terminal, and no progress bars.
- `init` is the in-band way to build; its summary includes the new index's `commit` and `dirty`.
- A running `mcp` server holds a copy of the index it loaded; restart it after upgrading (see below).

## 🔢 Deterministic Order (v0.4.0+)

Every list has a total order, and target selection is deterministic (when several nodes match a name, the first by path, then line, is used and the result says which). Two builds of the same tree from different directory names give byte-identical output for every tool in both formats; `built_at`, `builder_pid` and the absolute `root` appear only in `status`.

| Tool | Order |
|:--|:--|
| `search`, `query` | path, line, name, kind, byte offset |
| `query` (file outline) | imports first (by imported path, line, name), then symbols in source order |
| `blast_radius`, `plan` | depth, path, line, name |
| `modules` | path |
| `imports` | path, line, module |
| doc tools | their existing orders, with path and byte offset as the final keys |

The file outline used to list symbols in reverse source order; it is now source order.

### Import resolution and the build guarantee (v0.4.1+)

The same tree always gives the same graph. Two builds of one tree, in one process or on two machines, give the same edges, the same `imports` table and (apart from `built_at`, `builder_pid`, `root` and the Git fields) a byte-identical index file. Files are walked in name order and no `HashMap` or `HashSet` order reaches the graph, the side tables or any tool output.

An import is linked to a file in this order; the first strategy that succeeds wins:

1. **Exact name.** The import equals a file's dotted name (`src.app.config` for `src/app/config.py`).
2. **Relative path.** A relative import (`./util`) is resolved against the importing file, trying `.ts`, `.tsx`, `/index.ts`, `/index.tsx` and `/main.tf`.
3. **Suffix match** (Rust files skip it; they have their own `use` resolver). On whole dot-separated segments, never raw text, so `redis` does not match a module `myredis`. Tiers, best first: (1) the file's name ends with the import (`src.ost_manager.config` for `ost_manager.config`); (2) the import starts with the file's name (`pkg.mod.Thing` for `pkg/mod.py`); (3) both have two or more segments and their last two segments are equal. Within a tier the file with the fewest extra leading segments wins. **If two or more files are still tied, the import is left unlinked** (`resolved: false` in `imports`) rather than guessed. Build with `PROJECT_MAP_LOG=debug` to see the first 20 ties and the total on stderr.

## 🔒 Concurrency & Watcher Safety (v0.2.0+)

From version `0.2.0`, `project-map-cli-rust` is fully multi-process and multi-agent safe:
- **Single-Writer Advisory Locking**: An advisory file lock (`<out_dir>/.lock`) prevents multiple simultaneous writers from conflicting during index generation and rotation.
- **Watcher Coalescing**: When multiple watchers (e.g. Claude Code and Antigravity agents running simultaneously on one workspace) receive file modification bursts, they acquire the lock in turn. Any watcher observing that an index was already rebuilt newer than the triggering event skips the rebuild and reloads immediately.
- **Pure & Resilient Readers**: Reader operations (`query`, `status`, `fetch`, etc.) are completely read-only and never write to the output directory. If an index is temporarily being updated, callers retry with exponential backoff (`load_with_retry`). If a reload encounters an error, the in-memory engine retains the previous valid index.
- **Atomic File Operations**: Index snapshots and `latest/` pointers are updated via atomic rename operations with `fsync`, ensuring readers never observe partially written files.

## 🔄 Upgrading to 0.4.0

```bash
# 1. Update the binary
cargo install project-map-cli-rust --force

# 2. Restart EVERY running `project-map-cli-rust mcp` process (all editors, agents and
#    terminals, in every repository).

# 3. Rebuild each workspace index once
project-map refresh
```

The index `schema_version` is now 4 (Git fields and the imports table). A 0.3.0 watcher refuses to overwrite a schema-4 index (its forward guard logs it and skips the rebuild), so it cannot damage one, but it also cannot read the Git fields or imports: **restart the servers after upgrading** so they load the new binary. Until step 3, a schema-3 index keeps working for code and doc tools, is reported as stale, has `commit: null`, and `imports` returns `index_stale`.

### Compatibility

- Tool names and existing parameters are unchanged. New optional parameters only: `format` (all tools), `limit` (`search`, `query`, `blast_radius`, `plan`) and `max_depth` (`status`, `doc_outline`). `modules` and `imports` are new tools.
- TOON content is the 0.3.0 content, except: list order (see above), `blast_radius` (dependents rather than callers, the target's own file no longer listed, direct and transitive counts added), Rust `context` output (new import entries, now that `use` resolves) and the notes named in this README. Every error now also sets `is_error`.

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
