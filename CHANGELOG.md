# Changelog

All notable changes to `project-map-cli-rust` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
