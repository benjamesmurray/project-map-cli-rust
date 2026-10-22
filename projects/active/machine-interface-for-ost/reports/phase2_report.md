# Phase 2 Report: Index Additions (machine-interface-for-ost)

Date: 2026-10-22. Gate task: 2.6. Next: Phase 3, after the owner's approval.

## Built

| Task | Result |
|:--|:--|
| 2.1 Schema 4 | `SCHEMA_VERSION = 4`. `IndexMeta` gains `git_commit`, `git_dirty`, `git_branch`; new `ImportEntry` and the `imports` side table on `ProjectGraph` and `SerializableGraph` (all `#[serde(default)]`; the table is omitted from JSON when empty). A schema-3 index loads, is reported stale and is not rewritten; a schema-5 index makes a build fail with `NewerIndex` and stays byte-identical. |
| 2.2 `core/git.rs` | `read_git_state(root, GitBudget) -> GitState { commit, dirty, branch, error }` over `git rev-parse HEAD`, `git status --porcelain`, `git symbolic-ref --short -q HEAD`. `GitBudget::BUILD` is 5 s per call; `GitBudget::QUERY` is one 1.5 s budget for all calls. Every call sets `GIT_OPTIONAL_LOCKS=0`. Any failure gives `None` for that field; a Git that runs but fails (e.g. "dubious ownership") logs a warning and returns its first stderr line in `error`. The orchestrator stores the state in `IndexMeta` at build start. |
| 2.3 JavaScript and new captures | `js`, `jsx`, `mjs`, `cjs` parse with the TSX grammar (no ERROR nodes on the fixtures, so `tree-sitter-javascript` was not added) and index as `typescript`. New captures: `require('x')` and `import('x')` with a string literal in JS/TS; Rust `use` (full path text, whitespace collapsed, one record per statement) and `extern crate`. Skip rules: `*.min.*`, over 256 KB, or a line over 1,000 chars in the first 32 KB; skipped files get no node and no error line. |
| 2.4 Import records | `FileOutline.imports` is `Vec<ImportRecord { module, line, text }>` (1-based line, trimmed line text cut to 200 chars). `package_name(language, module)` implements the Specification's rules for Python, JS/TS, Rust, PHP, Kotlin and Lua; HCL and GDScript give none. |
| 2.5 Imports table | Pass 2 records every import as an `ImportEntry` (sorted by path, line, module) with `resolved` and `package`. Existing resolution strategies are unchanged for non-Rust languages. New `core/rust_use.rs`: `crate::`, `self::`, `super::` (including repeated `super`) and the crate's own name resolve to `Imports` edges (symbol in the file, else the file; each leaf of a grouped use; globs and `as` handled). The crate root is the nearest ancestor `Cargo.toml`; its `[package] name` (`-` as `_`) is the own-crate name and gives `package: None`. |

## Tests

- Before: 210 passing. After: **250 passing, 0 failed**. `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
- New: `tests/git_state_test.rs` (12: clean, edited, untracked, ignored, `.project-map` not dirtying, detached HEAD, not a repository, no commits, Git missing via empty PATH, fake `git` for `GIT_OPTIONAL_LOCKS=0`, exit 128 with `error`, timeout, shared query budget), `tests/javascript_index_test.rs` (8), `tests/imports_table_test.rs` (8, including edge counts by type for 13 existing non-Rust fixtures taken from the 0.3.0 binary), unit tests in `graph.rs` (schema-3 load, schema-4 round trip, a 0.3.0-shaped reader parsing a schema-4 index), `parser.rs` (package rules, import records) and `rust_use.rs` (4); `regression_test.rs` gained a schema-3 never-rewritten test.
- **This repository, same tree, 0.3.0 binary vs new binary:** `Imports` edges 17 to 215 (total edges 1,603 to 1,813; files 130 to 135, the five new JS fixture files). The side table holds 332 records, 152 resolved. The rise is Rust `use` resolution plus the new JS files; no other edge type moved apart from `Contains`/`References` for those five files.

## Deviations from the Specification, and why

1. **Two existing tests were touched.** `tests/integration_test.rs` pinned `schema_version == 3` in `test_index_meta_population_and_persistence`; schema 4 makes that line change to 4 (unavoidable). In `tests/regression_test.rs`, `test_forward_guard_refuses_to_overwrite_newer_index` gained one assertion (`found == 5`). No other 0.3.0 test changed. Phase 1's "no test edited" no longer holds for this reason only.
2. **"Stale" is split in two.** `is_stale()` now means schema < 4 (as 2.1 requires), but doc tools and the doc counts depend on schema >= 3, so a new `lacks_docs()` (schema < 3) guards them. A schema-3 index keeps working doc tools and `status` shows a new `OLD_SCHEMA_MESSAGE` ("predates Git and import records") instead of `STALE_INDEX_MESSAGE`. Without this, every 0.3.0 index would have lost its doc tools until a rebuild.
3. **`git status` ignores `.project-map`.** The pathspec is `-- . :(exclude).project-map`, so the index directory under the root does not make the tree dirty on the next build. Custom out directories inside the root would still count.
4. **Rust imports skip the legacy resolution strategies.** Strategy 3 (FQN suffix match) would link `use` paths to unrelated files by suffix, so Rust uses only `rust_use`. `crate::` is resolved only for files under `<crate>/src/` (not `src/bin/`); in `tests/` only the crate's own name resolves.
5. **Unspecified rules added.** URL imports (`https://…`) have no package. A repository without commits gives a `None` commit and no `error`.
6. **ImportEntry language is the parser's language,** so JS files are `typescript`, as for symbols.

## Open items carried forward

- Phase 1 items still apply: amend the Data Models table (2.x contract test), populate `commit`/`dirty` in `IndexInfo`/`StatusResult` from the new meta fields, wire `limit` into tools, make `depth`/`direct`/`transitive`/`files_affected` always present in 3.x.
- `read_git_state` with `GitBudget::QUERY` is built and tested but not called yet (3.1).
- The TOON byte-identity comparison from Phase 1 was not repeated: the new binary indexes extra files and edges, so output differs from 0.3.0 on this tree by design.
- Nothing in OST-Manager was touched; `~/.cargo/bin` is still 0.3.0.
