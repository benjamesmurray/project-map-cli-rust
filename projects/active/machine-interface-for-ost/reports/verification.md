# Verification (machine-interface-for-ost)

Date: 2026-10-22.

## 6.1 Performance, size and output size

Method as in `baseline.md`: release builds of 0.3.0 (`4d4a6e9`, scratch worktree) and the current tree, each run against the **same current tree** and the same `/opt/wde`; cold full build into a scratch `--out`, median of 3; index JSON size in bytes. (The 1.1 numbers were taken on the tree as it was then; the comparison below is like for like.)

### First measurement: two breaches

| Repository | 0.3.0 time (ms) | 0.4.0 time (ms) | Change | 0.3.0 size | 0.4.0 size | Change |
|:--|--:|--:|--:|--:|--:|--:|
| this repo | 371 | 511 | **+38 %** | 579,032 | 707,462 | **+22 %** |
| /opt/wde | 4,615 | 5,062 | +9.7 % | 14,506,171 | 15,994,174 | +10.3 % |

Limits: time <= +10 %, size <= +15 %. This repository breached both, so each became a fix.

### Fixes

1. **Time: compiled tree-sitter queries are cached per extension** (`CodeParser.queries`). 0.3.0 compiled the language's query for every file; 0.4.0's Rust query is larger (it captures `use` and `extern crate`), and this repository is mostly Rust files. Behaviour is unchanged.
2. **Size: the index JSON is written compact, not pretty-printed** (`serde_json::to_string` in `ProjectGraph::save`). Readers parse both, including 0.3.0's; the compact index is smaller than the pretty one it replaces. Like-for-like, the data grows by the amounts in the last two columns (both sizes measured as compact JSON), and that growth is the imports table and the Rust `Imports` edges.

### Result

| Repository | 0.3.0 time (ms; 3 runs) | 0.4.0 time (ms; 3 runs) | Change | 0.3.0 index (as written) | 0.4.0 index (as written) | Change | Like-for-like (compact) 0.3.0 | 0.4.0 | Data growth |
|:--|:--|:--|--:|--:|--:|--:|--:|--:|--:|
| this repo (191 files) | 394, 394, 388 → **394** | 265, 266, 267 → **266** | **-32 %** | 579,119 | 476,842 | **-18 %** | 386,841 | 477,003 | +23 % |
| /opt/wde | 4,566, 4,633, 4,645 → **4,633** | 2,018, 2,105, 2,054 → **2,054** | **-56 %** | 14,518,440 | 11,459,760 | **-21 %** | 10,304,999 | 11,461,805 | +11 % |

Both limits are met on the as-written measure the 0.3.0 baseline used (time <= +10 %, size <= +15 %). Said plainly: the on-disk size is below 0.3.0 because of the compact format; the data itself grew by 23 % on this repository, over the 15 % limit if the format were held fixed. That is the imports table (427 records here, about 87 KB pretty-printed) plus the new Rust `Imports` edges, which the Specification adds on purpose and says are bounded by the source.

### TOON output, 14-call set (this tree, scratch indexes built by each binary)

Characters of output, `Root`, `Built At`, `Index`, `Commit` lines ignored.

| Call | 0.3.0 | 0.4.0 | |
|:--|--:|--:|:--|
| `status` | 2255 | 2255 | differs only in counts (+5 files, +10 symbols, +296 edges from the new JavaScript fixtures and Rust `use` edges) and the new `Commit:` line, which the Specification allows |
| `search graph` | 508 | 504 | list order; the `Next Step` hint names the first match |
| `context -p src/core/graph.rs` | 2183 | 2253 | source order, imports first (list order allowed) |
| `context -p src/core/query_engine.rs` | 1678 | 2118 | as above, plus Rust imports that now resolve (allowed) |
| `blast -p src/core/query_engine.rs -s load` | 373 | 1032 | `blast_radius` change (item 5): 0.3.0 listed the target's own file, 0.4.0 lists the method's dependents through its type |
| `find -q load` | 1293 | 1331 | sorted, new Rust matches |
| `find -f core` | 358 | 371 | sorted by path |
| `impact -f QueryEngine` | 65 | 65 | byte-identical |
| `fetch -p src/main.rs -s main` | 7319 | 7319 | byte-identical |
| `doc outline README.md` | 1596 | 1596 | byte-identical |
| `doc terms` | 3967 | 3967 | byte-identical |
| `doc search blast` | 2969 | 2969 | byte-identical |
| `doc links README.md` | 3931 | 4066 | the new JS fixture file's `app.js::total` is a new bare-name match and the README has new sections; no change of format |
| `api` | 555 | 555 | byte-identical |

Everything that differs is list order, `blast_radius`, new Rust imports, or content the new files and README add, as the Specification allows. `cargo test`: 339 passed, 0 failed, after both fixes.

## 6.2 Quality gate and coverage map

Gate (after the 6.1 fixes): `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean; `cargo test` **339 passed, 0 failed**, 15 doctests ignored (watcher, regression, server-mode and spawned-binary suites included).

Tests are named by file and function. Doc and design items without a test say so.

### In-scope items

| # | Item | Tasks | Tests / evidence |
|:--|:--|:--|:--|
| 1 | JSON output, envelope, `format` on every tool and subcommand, `structured_content`, no `outputSchema`, no character cut, `limit` defaults, `index.state` | 4.1-4.4 | `json_out_test` (all six), `mcp_format_test::every_tool_answers_in_both_formats`, `::every_tool_declares_format_without_an_output_schema`, `::limit_caps_toon_lists_and_says_so`, `cli_format_test` (8), `json_contract_test` (39 goldens) |
| 2 | Indexed commit, dirty, branch, `GIT_OPTIONAL_LOCKS=0`, timeouts, `git_error`, `at_head`, schema-3 `commit: null` | 2.1, 2.2, 3.1 | `git_state_test` (12, incl. `every_git_call_sets_optional_locks_off`, `git_that_outlives_its_budget_*`, `query_budget_is_shared_*`), `status_test` (clean, edit, new commit, dirty build, no repository) |
| 3 | `max_depth` on `doc_outline` and `status`, `truncated` | 3.1, 3.2, 4.2 | `status_test::max_depth_cuts_the_tree_and_says_so`, `::doc_outline_*` (3), `mcp_format_test::max_depth_cuts_status_and_doc_outline_and_says_so` |
| 4 | `modules` tool and subcommand | 3.3, 4.2, 4.3 | `modules_test` (7), `mcp_format_test::modules_and_imports_have_toon_forms` |
| 5 | Blast radius as numbers (dependents, depths, summary, `matched`, languages, TOON summary) | 3.4 | `blast_test` (16), `json_out_test::imports_and_blast_on_the_imports_fixture`, goldens `blast_radius_*` |
| 6 | `imports` tool; records; package names; JS indexing, `require`/`import()`; Rust `use` and resolution; JS skip rules | 2.3-2.5, 3.5 | `parser.rs` unit tests `test_package_name_*` (4) and `test_import_record_*`, `javascript_index_test` (8), `imports_table_test` (8), `imports_query_test` (10), `imports_fixture_test` (6), `rust_use.rs` unit tests (4) |
| 7 | Server mode | 4.5 | `server_mode_test` (4, spawned binary), `mcp_format_test::a_missing_index_gives_typed_errors_except_for_status` |
| 8 | Deterministic order and target selection | 3.6 | `determinism_test` (8) |
| 9 | Agent ergonomics: TOON default, tool descriptions, `format` wording, server instructions | 4.2 | `mcp_test::test_mcp_every_tool_description_names_its_use_case`, `mcp_format_test::initialize_carries_instructions`, `::every_tool_declares_format_*` |
| 10 | Tool deferral finding | 1.5, 5.1 | `reports/tool-deferral.md`; README "Using with Claude Code". Not trialled in a fresh session (stated in both). |

### Technical constraints

| Constraint | Evidence |
|:--|:--|
| Rust 2021, Git via `Command`, no `git2`, `serde_json`, TSX grammar for JS | `Cargo.toml`; CHANGELOG records the grammar choice; `javascript_index_test::javascript_fixtures_parse_without_error_nodes` |
| Schema 4, `#[serde(default)]` additions; schema-3 loads read-only, stale; code and doc tools keep working; `imports` gives `index_stale` | `graph.rs::test_schema_3_index_loads_stale_with_empty_imports`, `regression_test::test_schema_3_index_is_stale_and_never_rewritten`, `imports_query_test::a_schema_3_index_is_index_stale`, `mcp_format_test::a_stale_index_gives_index_stale_*` |
| 0.3.0 watcher refuses a schema-4 index; 0.3.0-shaped struct reads schema 4 | `regression_test::test_forward_guard_refuses_to_overwrite_newer_index`, `graph.rs::test_forward_compatibility_0_3_0_reader_with_v4_index` |
| Imports side table bounded by source | one record per import statement (`imports_table_test::side_table_is_sorted_*`); size in 6.1 |
| Indexing time <= +10 %, size <= +15 % | 6.1 above (met after two fixes; data growth 23 % on this repository, stated there) |
| No change to tool names or parameters; only `format`, `limit`, `max_depth` added | `mcp_test::test_mcp_existing_tool_schemas_are_unchanged` (reworked to allow only these) |
| TOON unchanged beyond order and `blast_radius`; Rust `context` imports | 6.1 call table; Phase 1 byte-identity check (13 calls) |
| No network access | no network crate in `Cargo.toml`; Git runs locally |

### User stories

| Story / criterion | Evidence |
|:--|:--|
| Envelope for every tool; `structured_content` equals text | `mcp_format_test::every_tool_answers_in_both_formats` |
| Goldens fail on added, removed or renamed field | `json_contract_test::the_diff_shows_added_removed_and_changed_lines`, `::json_output_matches_the_golden_contract_files` |
| Clean repo: commit equals HEAD, not dirty, `at_head` | `status_test::clean_tree_at_the_indexed_commit` |
| After an edit dirty and not at head; after a new commit not at head; no repository gives nulls and the build succeeds | `status_test::an_edit_*`, `::a_new_commit_*`, `::outside_a_repository_*`; `git_state_test::not_a_repository_*`, `::git_missing_*` |
| `max_depth=1` for `doc_outline` and `status`, with truncation | as item 3 |
| `modules(max_depth=1)` sums to the root | `modules_test::children_sum_to_the_parent_minus_its_own_files`, `::the_root_counts_match_the_index_meta` |
| `blast_radius` fields and `depth`; Rust crate importers at depth 1; own file not listed | `blast_test::rust_crate_importers_appear_through_use_crate`, `::the_targets_file_and_enclosing_symbols_are_never_listed`, `::summary_counts_cover_every_dependent` |
| `imports` for `@sentry/*`, `stripe`, `sentry_sdk`, `serde_json` across JS, TS, Python, Rust | `imports_query_test::scoped_packages_match_by_prefix_across_languages`, `::an_exact_package_lists_its_import_lines` |
| Empty directory `status` within 2 s, no ANSI on stderr, no watcher without `-w` | `server_mode_test` (4) |
| Byte-stable output across two directories, both formats | `determinism_test::*_identical_across_directories_*`, `::typed_results_are_identical_across_directories` |
| Every 0.3.0 test passes unchanged except those listed | Phase 2 and 4 reports list the edits: `integration_test` (schema 4), `regression_test` (one assertion), `mcp_test` (3 tests), `typed_results_test` (arguments, option line), `DummyRuntime` moved to `tests/common` |
| TOON sizes recorded before and after, none grows except `blast_radius` and Rust `context` | 6.1 table |

### Verification criteria

| # | Evidence |
|:--|:--|
| 1 Contract | goldens for the docs and imports fixtures (39), `mcp_format_test`, 0.3.0 MCP tests pass |
| 2 Git | `git_state_test`, `status_test` |
| 3 Depth and modules | `status_test`, `modules_test`, `mcp_format_test` |
| 4 Blast radius | `blast_test` (diamond, summary, `Contains` ancestors, file importer, `matched: file`, Rust crate) |
| 5 Imports | `imports_*`, `javascript_index_test`, parser unit tests |
| 6 Server mode | `server_mode_test` |
| 7 Determinism | `determinism_test` (includes ambiguous `plan`, `blast_radius` and `fetch_symbol` targets) |
| 8 Compatibility | schema-3, forward-guard and 0.3.0-reader tests (above) |
| 9 Performance | 6.1 |
| 10 Release | README and CHANGELOG (Phase 5), fmt and clippy (this gate); version bump, publish dry-run and hand-off are 6.3; publish and install wait for the owner |

### Gaps

None that need a task. Not covered by an automated test, by design: the README's `alwaysLoad` claim (documented behaviour, not trialled) and the post-release OST-Manager check, which the Specification says is not a release gate.
