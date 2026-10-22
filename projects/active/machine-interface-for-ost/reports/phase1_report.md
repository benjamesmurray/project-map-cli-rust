# Phase 1 Report: Foundations (machine-interface-for-ost)

Date: 2026-10-22. Gate task: 1.6. Next: Phase 2, after the owner's approval.

## Built

| Task | Result |
|:--|:--|
| 1.1 Prerequisites and baseline | Only 0.3.0 servers run (PIDs 1861039, 1962498). 0.3.0 baseline for this repo (224 ms, 341,914 B index) and `/opt/wde` (4,658 ms, 14,480,105 B index) in `reports/baseline.md`; six call outputs in `reports/baseline/`. |
| 1.2 Fixtures | `tests/fixtures/imports/` (JS `.js/.jsx/.mjs/.cjs`, TS, Python, PHP, diamond, `rust_crate/`, skipped-JS cases, README mapping file to rule); `tests/common/mod.rs` with `copy_fixture`, `git_fixture` (deterministic commit) and `write_large_js`; `tests/imports_fixture_test.rs`. |
| 1.3 Result types | `src/core/results.rs`: a `Serialize` struct for every tool, `QueryError` / `ErrorCode`, `IndexInfo`, `QualifiedNamer`, `end_line_of`. |
| 1.4 Query / TOON split | `src/core/typed_queries.rs`: a typed result for every tool, built from the same node-level queries TOON renders. TOON output unchanged. |
| 1.5 Deferral finding | `reports/tool-deferral.md`: Claude Code defers MCP tools by default; server `instructions` are shown at session start; `alwaysLoad` (user, per server) and `anthropic/alwaysLoad` (server, per tool) change it. |

## Tests

- Before: 178 passing. After: **210 passing, 0 failed**. `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
- New: 12 unit tests (`results.rs`), 6 (`imports_fixture_test.rs`), 14 (`typed_results_test.rs`).
- **No 0.3.0 test was edited.** `git diff` over existing test files is empty; the only changes under `tests/` are new files.
- **TOON byte-identical:** the 0.3.0 binary (built from `4d4a6e9` in a scratch worktree) and the new binary, both run against the same tree, give identical output for 13 CLI calls (status, search, context, blast, find x2, impact, fetch, doc search / outline / links / terms, api). The saved `reports/baseline/*.toon` are of the older tree, so they are the reference for sizes, not for bytes (see `baseline.md`).

## Deviations from the Specification, and why

1. **Results are built beside the renderers, not under them.** 1.4 says toon.rs renders the result structs. Two facts rule that out here: 0.3.0's tests call the node-level API directly (`find_symbols`, `check_blast_radius`, `get_file_outline`, `format_entity_matches(&[EntityMatch])`, ...) and must pass unedited, and TOON prints the bare symbol name where the result carries the qualified one (`AuditLog.append`). So `typed_queries.rs` builds each result from the same node-level query, in the same order, and the Phase 2 JSON renderer consumes it. The doc tools were already split into a query returning a view and a renderer, so they were extended (heading level, line, score, end line, API line) rather than rewritten.
2. **Fields not computable yet are optional.** `depth` (blast_radius, plan items), `direct`, `transitive` and `files_affected` are `Option` and omitted until the breadth-first walk (Phase 3) produces them. 3.x must make them always present and the contract test (2.x) must pin them then.
3. **Fields added to the Data Models table** (the table in the Specification should be amended in 2.x): `role` and `path` on `query` path-form items for import entries (0.3.0 lists them); `truncated` on every list result; `requested` on `plan`; `end_line` on `blast_radius` items; `total` on `doc_links` and `api_surface`.
4. **`outline_result` returns `not_found` for an unindexed file** (TOON still prints its "(No symbols detected or file not indexed)" line).
5. **`limit` exists in the builders** (`entity_list`, `files_result`, `blast_result`, `plan_result`) but is not yet an argument of any tool; that is Phase 2.
6. **Small supporting changes:** `NodeType` gained `Eq` and `as_str()` (snake_case, for JSON); `render_tree` now sits on a `PathTree` that also yields the structured tree (output identical); `blast_target`, `plan_target`, `outline_file_node` and `openable_path` were factored out of existing queries (behaviour unchanged); the server's "Index not loaded" and "Provide query, path, or filename" strings now come from `QueryError` (text unchanged).
7. **1.5 was not tested in a fresh session** (documented behaviour read in full, this session's state used as evidence); the report says how to verify after the owner applies `alwaysLoad`.

## Open items carried forward

- `tests/fixtures/imports/rust_crate/Cargo.toml` is a nested package inside the crate. `cargo build` and `cargo test` ignore it; check `cargo package --list` and add an `exclude` if needed in 5.x.
- The commit is pushed with this gate. Nothing in OST-Manager was touched.
