# Changelog

All notable changes to `project-map-cli-rust` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
