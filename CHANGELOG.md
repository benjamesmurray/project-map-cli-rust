# Changelog

All notable changes to `project-map-cli-rust` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-10-22

### Upgrade note (read first)
The index `schema_version` is now 4. After installing 0.4.0, **restart every running `project-map-cli-rust mcp` process** and run `project-map refresh` once per workspace. A 0.3.0 watcher refuses to overwrite a schema-4 index (its forward guard logs it and skips the rebuild), so it cannot damage one, but it cannot read the Git fields or imports either. A schema-3 index still serves the code and doc tools, is reported as stale with `commit: null` and is never rewritten by a query; `imports` returns `index_stale` on it.

### Breaking
- **`blast_radius` now reports dependents, not callers.** The graph has no call edges. The result is a breadth-first walk over incoming `Imports`, `Produces`/`Consumes` and `Configures` edges, each dependent once at its shortest `depth` (1 is direct). The target's own file and enclosing symbols are no longer listed (0.3.0 listed the target's own file as a dependent), so counts change. The TOON `Summary:` gains `Direct Dependents: n | Transitive: m`, and `Unique Files Affected` is the number of distinct dependent files.
- **List order changed.** Every list now has a total order: `search`/`query` by path, line, name; `blast_radius`/`plan` by depth, path, line, name; a file outline lists imports first and then symbols in source order (it was reverse source order). Target selection is deterministic too: the first match by path, then line.
- **Every error now sets `is_error`** on the MCP result, in TOON as well as JSON. The error text is unchanged.
- `fetch_symbol` reads the file relative to the index root rather than the server's working directory, and a byte range outside the file is reported as an error instead of falling back to a line-based snippet.

### Added
- **JSON output.** `format: "json"` on every MCP tool and `--format toon|json` on the CLI. A stable envelope (`tool`, `contract`, `index`, `truncated`, `total`, `summary`, `results`), the error codes `index_missing`, `index_stale`, `not_found`, `invalid_argument` and `io_error`, and the same object in `structured_content`. `contract` is 1 and moves independently of `schema_version`; 39 golden envelopes pin it. JSON is never cut by characters. TOON stays the default.
- **`limit`** on `search`, `query`, `blast_radius` and `plan` (JSON defaults 50, 50, 200, 200). In TOON a given `limit` cuts the list and appends `[limit N: M matched]`; omitted, TOON behaves as in 0.3.0.
- **`max_depth`** on `status` and `doc_outline`. TOON appends `[tree cut at depth N]` or `[outline cut at depth N]` when it cut something.
- **Git state.** A build records the commit, whether the tree was dirty and the branch. `status` adds `head_now`, `dirty_now`, `at_head` and `git_error`. Git runs with `GIT_OPTIONAL_LOCKS=0` and a timeout (5 s per call at build, one 1.5 s budget at query); without Git or a repository the fields are `null` and builds succeed. `git status` ignores `.project-map`, so building does not make the tree dirty.
- **`modules` tool and CLI subcommand**: recursive `files`, `code_files`, `symbols`, `doc_files` and `doc_sections` per directory.
- **`imports` tool and CLI subcommand**: every import of an external package with its line, text, package name and `resolved` flag. The index keeps all imports in a side table, resolved or not. Package-name rules per language are in the README.
- **JavaScript** (`.js`, `.jsx`, `.mjs`, `.cjs`): symbols and imports. Grammar choice: the existing `tree-sitter-typescript` TSX grammar parsed every JavaScript fixture without ERROR nodes, so `tree-sitter-javascript` was not added. JavaScript files are reported as language `typescript`. `require("x")` and `import("x")` with a string literal are captured in JavaScript and TypeScript.
- **Built and minified JavaScript is skipped** (no symbols, no imports, no error line): a file named `*.min.*`, larger than 256 KB, or with a line over 1,000 characters in its first 32 KB. TypeScript is not affected.
- **Rust imports.** `use` declarations and `extern crate` are captured (one record per statement). `crate::`, `self::`, `super::` and own-crate-name paths resolve to `Imports` edges (to the symbol, else the file; each leaf of a grouped `use`; globs and `as` handled), so `blast_radius` now works on Rust. The crate root is the nearest ancestor `Cargo.toml`. Rust files skip the suffix-match resolution strategy, which would link `use` paths to unrelated files. On this repository `Imports` edges rise from 17 to 215.
- **Blast radius seeding.** A `matched: file` target also starts from the file's symbols, and a method is counted through its enclosing symbol (the summary's `via`, with a TOON note), because no import names a method.
- **Server instructions** sent in the MCP initialize result, so a client that defers the tools still shows the model what the index covers. README section "Using with Claude Code" records the deferral finding and the settings (`alwaysLoad`, `ENABLE_TOOL_SEARCH`).
- **Status** TOON adds a `Commit: 1a2b3c4 (clean) | at_head: yes` line.
- Internals: `core/json_out.rs` (envelope) and `core/tools.rs` (one dispatcher used by the MCP server and the CLI, so the two cannot drift apart).

### Changed
- `status` with a schema-3 index shows "predates Git and import records" and keeps the doc tools working.
- A missing index gives `index_missing` from every tool except `status`. A stale one gives `index_stale` only where data is absent (`imports`, doc tools on schema < 3); code tools keep answering.
- The tracing subscriber writes ANSI escapes only when stderr is a terminal.
- `NodeType` has `Eq` and `as_str()`.

### Server mode (verified by spawned-binary tests)
- `mcp` without `-w` starts no watcher and never builds on its own; `status` answers within 2 s on an empty directory; stdout carries only JSON-RPC frames; stderr has no ANSI escapes.

## [0.3.0] - 2026-10-22

### Upgrade note (read first)
After installing 0.3.0, **restart every running `project-map-cli-rust mcp` process** (all editors, agents and repositories) **before the first 0.3 build**. A 0.2.x watcher rebuilds the whole index on any change and would overwrite a schema-3 index with schema 2, dropping the documentation index. 0.3.0 cannot guard against 0.2.x binaries that are still running. Then run `project-map refresh` once per workspace.

### Added
- **Documentation indexing**: Markdown heading sections with GitHub-style anchors and byte ranges (section text is read from disk on demand, not stored), YAML frontmatter (`id`, `title`, `status`, `date`, `domain`, `tags`, `supersedes`), and recurring domain terms per directory.
- **Doc → code links**: exact, ordered resolution of Markdown links, paths, `path::symbol`, `path:line`, `Type::member` and unique bare names to `References` edges from the deepest section. Unresolved and ambiguous mentions are counted.
- **OpenAPI/Swagger**: detection by name or by a top-level `openapi`/`swagger` key (YAML/JSON up to 1 MB); operations and schemas are indexed, and detected specs no longer produce generic YAML symbols.
- **Six MCP tools**: `doc_search`, `doc_outline`, `doc_section`, `doc_links`, `doc_terms`, `api_surface`, with TOON output capped at 4,000 characters and a truncation marker.
- **CLI**: `doc search|outline|section|links|terms` and `api`, printing the same output as the MCP tools.
- `status` reports doc files, sections, references, unresolved and ambiguous mentions, and API operations.
- Index `schema_version` 3 with `serde(default)` count fields and `doc_meta` / `doc_terms` side tables. New `NodeType` variants `DocSection`, `ApiOperation`, `ApiSchema`.
- Dependency: `pulldown-cmark` 0.13.

### Changed
- All 13 MCP tool descriptions now say what question each tool answers better than grep. Tool names, parameters and output are unchanged (the existing seven tools' parameter schemas are snapshot-tested).
- An index with schema version below 3, or with no metadata, is loaded read-only: it is reported as stale and never rewritten by a query.
- A build refuses to overwrite an index with a newer `schema_version` (`AppError::NewerIndex`); the watcher logs it and skips the rebuild.
- Version strings such as `3.0.0`, `v1.2` or `1.0.0-rc1` are no longer indexed as Kafka topics (YAML, config and code string literals). Topic names with digits, such as `orders.v1` or `events.2024.audit`, are unchanged.
- Generic YAML key symbols now report their own line and byte range; a key repeated under several parents (or named like a comment or a longer key) no longer takes the location of its first text match.
- Publishing an index when `.project-map/latest/` is a real directory (written by older builds) no longer fails with `EISDIR`. It is migrated to the `latest` symlink without a moment at which `latest` is missing (atomic `renameat2` exchange on Linux; elsewhere the old directory is moved aside and removed). Adds a Linux-only `libc` dependency.
- Existing code tools (`query`, `search`, `plan`, `blast_radius`, `fetch_symbol`, symbol counts) skip documentation and API nodes. File outlines no longer list a document's references to code.

### Performance
- Full-index time on `/opt/wde` (1,628 Markdown files) is unchanged (4.5 s); on this repository +5 %. Index size grows (`/opt/wde` 7.5 MB to 13.7 MB) because sections, terms and references are stored.

## [0.2.0] - 2026-10-12

### Fixed
- **Corrupt-Index Recovery Root (D1, D2)**: `Graph::load` and `QueryEngine::load` are now strictly read-only. Parse errors return `AppError::CorruptIndex` instead of attempting an erroneous rebuild against `.project-map` which wiped the index. Callers utilize `load_with_retry` with exponential backoff.
- **Atomic Index Persistence (D3)**: Index files are saved via temporary files (`<target>.tmp.<pid>.<nanos>`) with `fsync`, then atomically renamed over the target. `latest` symlink updates are atomic via temporary symlink swap with directory-copy fallback.
- **Multi-Process Coordination & Single-Writer Lock (D4)**: Writers acquire an advisory single-writer lock (`<out_dir>/.lock` via `fs4`) around build, save, and rotation. Watchers debounce and coalesce events, skipping rebuilds if `latest` was already rebuilt newer than the triggering change.
- **Path Normalization (D5)**: Stored node paths are normalized to relative paths without `./` prefix (e.g. `src/main.rs`). All lookup APIs (`query`, `fetch_symbol`, `blast_radius`, `plan`) transparently normalize inputs accepting `./path`, relative `path`, and absolute paths identically. Legacy indexes with `./` paths are normalized in memory on load.
- **Status Visibility for Degenerate & Empty Indexes (D6)**: `status` reports `schema_version`, `tool_version`, `root`, `built_at`, file/symbol/edge counts, and active features. Emits actionable `WARNING` alerts for empty indexes, out-dir-only indexes, and root mismatches.
- **File Outline Mislabel (D7)**: Fixed file outline traversal to resolve the file's own node correctly and distinctly label import targets as `- import <name> (<path>)`.
- **Output Directory Exclusion (D8)**: Builder and watcher exclude output directories by canonical path, preventing recursive indexing of custom `--out` directories.

### Added
- **Index Metadata (`IndexMeta`)**: Added `meta` block to `.project-map.json` recording `schema_version`, `tool_version`, `root`, `built_at`, `builder_pid`, `file_count`, `symbol_count`, and `edge_count`. Backward and forward compatible with 0.1.x readers.
- Advisory lock file dependency: added `fs4 = "1.1.0"`.
- Dev-dependency: `tempfile = "3"`.

### Changed
- Node paths in index output and formatted TOON responses no longer start with `./`.
- MCP server and CLI commands share a unified `rebuild_and_publish` orchestrator pipeline and `format_full_status` reporter.
