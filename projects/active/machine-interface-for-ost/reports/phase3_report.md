# Phase 3 Report: New and Changed Queries (machine-interface-for-ost)

Date: 2026-10-22. Gate task: 3.7. Next: Phase 4, after the owner's approval.

## Built

| Task | Result |
|:--|:--|
| 3.1 Status | `status_result(max_depth, active_features)` reads Git at query time (`git_view()`, one 1.5 s budget) and sets `commit`/`dirty`/`branch` (from the index), `head_now`, `dirty_now`, `at_head` and `git_error`. The tree is cut at `max_depth` and `truncated` is set. `StatusResult::missing()` is the result for a missing index, with `next_step` (`refresh` or the `init` tool). `IndexInfo::loaded` now fills `commit`/`dirty` from the meta. TOON gains one line: `Commit: 1a2b3c4 (clean) \| at_head: yes`, plus `HEAD now: …` when the tree moved; nothing is printed when Git told nothing, and `Git: <error>` when it failed. |
| 3.2 doc_outline | `doc_outline_result(path, max_depth)` and `DocOutlineView::cut_to_depth`. Depth counts nesting (an h3 directly under an h1 is 2). `max_depth` 0 keeps nothing; none given equals the 0.3.0 output. |
| 3.3 modules | `modules_result(path_prefix, max_depth)`: recursive `files`, `code_files`, `symbols`, `doc_files`, `doc_sections`; root files under `.`; the prefix directory is listed at depth 0; sorted by path; an unknown prefix or a file gives `not_found`. `is_code_extension` added to the parser. |
| 3.4 Blast radius | 0-1 breadth-first walk (`QueryEngine::blast_walk`). Result carries `depth` on every item and `direct`, `transitive`, `total`, `files_affected`, `languages` (always present). `plan` is a breadth-first walk too and reports `depth`. `language_of_path` added to the parser. TOON `Summary:` gains `- Direct Dependents: n \| Transitive: m`, and `Unique Files Affected` is `files_affected`. The tool description and README say `direct` counts dependents, not callers. |
| 3.5 imports | `imports_result(package, path_prefix, limit)`: exact package, or prefix for `…/*` and `….*`; ordered by path, line, module; `files` and `imports` count every match; default limit 50; `invalid_argument` for an empty package, `index_stale` for a schema-3 index. |
| 3.6 Order | `find_symbols` (`search`, `query`): path, line, name, kind, byte offset. `find_files`: path. File outline: imports first (by imported path, line, name), then symbols by line. `blast_radius`, `plan`: depth, path, line, name. Target selection (`plan_target`, `blast_target`, `find_symbol_in_path`): first by path, then line. Doc tools were already total (see below). |

Doc-tool orders, checked and left as they were: `doc_search` (score, path, byte offset), `doc_outline` (document order), `doc_links` (path, byte offset, target), `doc_terms` (count, term; sections by path then stored byte order), `api_surface` (path, byte offset).

## Tests

- Before: 250 passing. After: **296 passing, 0 failed**. `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
- New files: `status_test.rs` (10: clean, edit after build, new commit, dirty build, no repository, `max_depth`, missing index, three `doc_outline` depth tests), `modules_test.rs` (7), `blast_test.rs` (11), `imports_query_test.rs` (10), `determinism_test.rs` (8).
- `determinism_test.rs` copies the docs and imports fixtures into two differently named directories, builds each, and compares 23 (docs) and 15 (imports) CLI calls (status included) byte for byte (`status` without `Root`, `Built At`, `Index` and `Commit` lines), then rebuilds one directory and compares again. A second test compares the JSON of over 30 typed results per fixture (every tool, the ambiguous names `flush_buffer`, `RetentionPolicy`, `apply`, `E`, `from`) across the two directories. The CLI emits TOON only, so JSON of the typed results stands in until 4.1.
- **0.3.0 tests changed: none.** The only edit to an existing test file is `tests/typed_results_test.rs` (a Phase 1 file): its three `doc_outline_result` calls gained the `max_depth` argument.
- Not reproducible: the old list order never differed between two directories on this machine (the file walk is not sorted but returns the same order on both), so the determinism tests would also have passed before 3.6. They guard the new explicit sorts; they are not a regression proof.

## `search` ordering: the 1.1 call set, before and after

0.3.0 binary and new binary, both run against the current tree (each built its own index), same commands as `baseline.md`. The new binary indexes more of this tree (Rust `use` edges, the JS fixtures), so sizes differ by design as well as by order.

| Call | 0.3.0 chars | Now | Difference |
|:--|--:|--:|:--|
| `search graph` | 535 | 531 | Same 13 matches, listed by path, line, name (0.3.0: build order, e.g. `tests/imports_table_test.rs:23` and `src/core/results.rs:824` before `src/core/docs.rs:294`). The `Next Step` hint now names `Collector`, the first match, not `fixture_graph`. |
| `search load` / `find -q load` | 795 | 835 | Sorted; new Rust-side matches. |
| `find -f core` | 366 | 371 | Sorted by path. |
| `context -p src/core/graph.rs` | 2184 | 2254 | Source order (0.3.0 listed symbols in reverse line order, 886 down to 14); imports first. |
| `context -p src/core/query_engine.rs` | 1679 | 2119 | Same, plus imports that now resolve. |
| `blast -p src/core/query_engine.rs -s load` | 374 | 98 | 0.3.0 listed the target's own file as the one dependent. Now `No dependent components found.` (see open item 1). |
| `impact -f QueryEngine`, `doc outline README.md`, `doc terms`, `doc search blast` | 67, 869, 3843, 2967 | same | Byte-identical. |

## Deviations from the Specification, and why

1. **Role edges are structure in the blast walk.** The Specification says `Produces`/`Consumes`/`Configures` edges cost 1. In this graph those edges (and `References`) only run from a file to the topic, table or environment variable *it contains*; they never link two files. Costing them 1 would list the target's own file at depth 1 for every topic, table and env var target (as 0.3.0 did). The walk therefore treats an edge from a file to a node of the same file as free, whatever its type; `Imports` always costs 1. Result: a topic target has no dependents, and its own file is never listed. Cross-file links come only from `Imports`.
2. **`plan` depth is the shortest distance in edges** (all edges cost 1, outgoing); the Specification says only that `plan` reports depth and its target. The reachable set is the one 0.3.0's DFS found.
3. **`languages` uses the parser's names** (`typescript` for JavaScript, `hcl` for `.tf`), as `ImportEntry.language` does.
4. **`doc_files` counts Markdown files** (what `IndexMeta.doc_file_count` counts), not OpenAPI files; `symbols` is `IndexMeta.symbol_count`'s definition (topics, tables and env vars included).
5. **`StatusResult` has `truncated`, `missing` (both `#[serde(skip)]`, for the envelope) and `next_step`** (serialised only when the index is missing). A missing index gives zero counts and `schema_version` 0 with `index.state` `missing` in the envelope.
6. **`doc_outline` and `plan`/`blast` list orders** changed as the Specification allows for whole lists. `get_file_outline` (the `context` order) is part of that: it was reverse source order and is now source order.
7. **Specification Data Models table amended** with the items carried from Phase 1/2: `role` and `path` on path-form `query` items, `requested` on `plan`, `total` on `doc_links` and `api_surface`, `next_step` on `status`, and two rules (every list has `truncated` and `total`; the blast summary counts are always present). `end_line` on `blast_radius` items was already in the table.

## Open items for the owner

1. **Rust method and file targets find few dependents.** `use crate::a::b::Sym` links the importing file to the *symbol* `Sym`, so a blast radius on a method (`QueryEngine::load`) or on the file (the `matched: file` fallback) finds importers of the file node only, which is rare in Rust. Following the Specification as written, `blast -s load` on `query_engine.rs` reports none, although the file is imported. Option: for a `matched: file` target, start the walk from the file and every symbol it contains (structure, cost 0). It is a few lines, changes no test, and would make file-level and method-level numbers useful for P8. Not done, because it goes beyond the Specification.
2. `get_file_outline` now sorts; nothing in 0.3.0's tests pinned its order, but agents that learned the reverse order will see source order.

## Carried forward

- Phase 4: `format`, `limit`, `max_depth` arguments on the tools and CLI, the `modules` and `imports` tools and subcommands, and the JSON envelope (`truncated` for `status` comes from `StatusResult.truncated`; `index.state` `missing` from `StatusResult.missing`). `format_full_status` and `render_tree` still use depth 3; wire `max_depth` through them in 4.x.
- `~/.cargo/bin` is still 0.3.0; running servers are 0.3.0. Nothing in OST-Manager was touched.
