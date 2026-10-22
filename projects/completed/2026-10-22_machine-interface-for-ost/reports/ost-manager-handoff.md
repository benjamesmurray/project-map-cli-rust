# Hand-off to OST-Manager: project-map 0.4.0

Date: 2026-10-22. Source brief: `OST-Manager/projects/p4-project-map-uplift.md` (items 1-10). Nothing in the OST-Manager repository was edited. Test names are in `tests/`; the full coverage map and measurements are in `reports/verification.md`.

## Items 1-10

| # | Brief item | 0.4.0 | Evidence |
|:--|:--|:--|:--|
| 1 | Structured output | `format: "json"` on all 15 MCP tools, `--format json` on the CLI, one envelope (`tool`, `contract: 1`, `index`, `truncated`, `total`, `summary`, `results`), error codes `index_missing`, `index_stale`, `not_found`, `invalid_argument`, `io_error`; MCP `structured_content` equals the text. Field names in the README table. A change to the contract is a breaking change (CHANGELOG rule, `contract` number). | 39 goldens (`json_contract_test`), `json_out_test`, `mcp_format_test`, `cli_format_test` |
| 2 | Index knows its commit | `status` shows `commit` (full SHA), `dirty`, `branch` from build time, and `head_now`, `dirty_now`, `at_head` at query time, `git_error` when Git fails. Refuse to cite unless `at_head` is true, or `commit` equals the pinned SHA. | `git_state_test`, `status_test` |
| 3 | GitHub anchors | Unchanged from 0.3.0 (`foo`, `foo-1`, `foo-2`). | 0.3.0 doc tests, unchanged |
| 4 | Depth controls | `doc_outline(path, max_depth)` and `status(max_depth)`; `truncated` says whether anything was cut; `doc_terms` keeps `limit`. JSON is never cut by characters, so the 12,000-character cap is the caller's to apply to a bounded list. | `status_test`, `mcp_format_test` |
| 5 | Module sizes | New `modules(path_prefix?, max_depth=2)` tool and subcommand: recursive `files`, `code_files`, `symbols`, `doc_files`, `doc_sections` per directory, sorted by path. | `modules_test` |
| 6 | Blast radius as numbers | `direct`, `transitive`, `total`, `files_affected`, `languages`, `matched`, and `depth` on every item. **Wording differs from the brief: these are dependents, not callers**, because the graph has no call edges. P8 should call it "dependents". Methods are counted through their enclosing type (`via`), and Rust now resolves `use crate::…`. | `blast_test`, goldens `blast_radius_*` |
| 7 | Who imports a package | New `imports(package, path_prefix?, limit=50)`: path, line, import text, package, language, `resolved`; exact or `…/*` prefix match. External packages were names, not nodes, so a side table holds every import. Covers JS/TS (`import`, `require`, `import()`), Python, Rust, PHP, Kotlin and Lua. JavaScript files are now indexed; built, minified or over-256 KB ones are skipped. | `imports_*`, `javascript_index_test` |
| 8 | Server mode for jobs | `mcp` without `-w` has no watcher and never builds on its own; `status` answers within 2 s on an empty directory and says the index is missing; other tools return `index_missing` with `is_error`; no ANSI bytes on stderr; stdout is JSON-RPC only; `init` builds in-band and returns `commit` and `dirty`. | `server_mode_test` (spawned binary) |
| 9 | Deterministic output | Every list has a total order; target selection is the first by path, then line; two builds from differently named directories give identical output in both formats (`status`'s build fields excepted). | `determinism_test` |
| 10 | Release through crates.io | Version 0.4.0, `cargo publish --dry-run` passes. **Not published**: waits for the owner. The brief's gate was "≥ 0.3.0"; use ≥ 0.4.0 now. | this report |

## Tool deferral finding (brief item 10 of the Specification, "Keep it good for coding work")

Claude Code defers MCP tools by default behind ToolSearch; at session start Claude sees only tool names and the server's `instructions`. 0.4.0 therefore sends instructions (about 1,300 characters) that name every tool and say to prefer them over grep. To load the tools at session start, set `"alwaysLoad": true` on the `map` server entry (or `ENABLE_TOOL_SEARCH=auto:N`). Full table in `reports/tool-deferral.md` and the README. Not trialled in a fresh session; check that `mcp__map__status` is callable without ToolSearch after applying it. This probably explains part of the map-versus-grep gap in the 2026-10-21 baseline.

## Measurements (0.3.0 vs 0.4.0, same tree)

Indexing time -32 % (this repository) and -56 % (`/opt/wde`); on-disk index -18 % and -21 %. Like-for-like data growth +23 % and +11 % (the imports table and Rust `Imports` edges). The brief's limit was 20 %; the Specification narrowed it to +10 % time and +15 % size. Details and the two fixes (query cache, compact JSON) are in `verification.md`.

## Proposed ROADMAP lines (OST-Manager is not edited)

- **P4 row:** `project-map 0.4.0: JSON envelope (contract 1), indexed commit and at_head, doc_outline/status max_depth, modules, blast_radius counts (dependents), imports, JavaScript and Rust import indexing, quiet server mode, deterministic order. Evidence: projects/active/machine-interface-for-ost/reports/ost-manager-handoff.md in project-map-cli-rust.`
- **P5 gate:** `released in project-map >= 0.4.0 (crates.io), and the Dockerfile installs it with cargo install project-map-cli-rust --version 0.4.0 --locked`.

## For P5's first task (not a release gate)

Run the 0.4.0 binary's JSON on a small fixture repository and on OST-Manager's `docs/`, and check that each tool's results map to citations that resolve. Cite with `path` **and** `line`: qualified symbol names are not unique (two `impl From<…> for E` blocks both give `E.from`). File nodes have no `line`.

## Waiting for the owner

Tag v0.4.0, `cargo publish`, `cargo install project-map-cli-rust --version 0.4.0 --locked --force`, restart the running 0.3.0 `mcp -w` servers, apply `alwaysLoad`.
