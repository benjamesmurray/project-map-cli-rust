# Phase 2 report

## Built
- **2.1** `orchestrator.rs` pass 2: Strategy 3 now calls `resolve_by_suffix` over an ordered `BTreeSet` of file FQNs (`fqn_to_node` is a `BTreeMap`). `Unique` links, `Ambiguous` and `None` leave `resolved = false`. Ambiguous imports are counted; the first 20 and the total are logged at `debug`. The CLI `build`/`refresh` show that log with `PROJECT_MAP_LOG=debug` (stderr; default level is warn). Strategies 1 and 2 and the Rust `use` resolver are unchanged.
- **2.2** Pass 1 walks with `WalkBuilder::sort_by_file_name`, so node and edge order depend only on the tree.
- **2.3** `HashMap`/`HashSet` audit (below). One real defect found and fixed beyond the specified one.
- **2.4** `tests/import_resolution_test.rs` (7 tests) and the new fixture added to `tests/determinism_test.rs` (both output formats, two directories, rebuild).

## Would the new tests have caught 0.4.0?
Yes. The installed 0.4.0 binary built `tests/fixtures/ambiguous_imports` 20 times and produced **4 distinct `Imports` edge sets** (the tie linked to `lib_a` or `lib_b` at random; `service.py` linked to `src/app/config.py` or to `tests/fixtures_copy/app/config.py`). It also linked `cache.py` (`import redis`) to `src/myredis.py` every time and never linked the tier-2 import. The 20-build test fails on any of these. The new `blast_summary_lists_equal_sized_directories_in_name_order` test fails on the pre-fix `toon.rs` (checked by reverting that file).

## HashMap / HashSet audit

| Site | Holds | Order can reach output? | Fix |
|:--|:--|:--|:--|
| `orchestrator.rs` `fqn_to_node` | file FQN to node | Yes (was `keys().find`) | `BTreeMap`; Strategy 3 gets an ordered `BTreeSet` |
| `orchestrator.rs` `path_to_node`, `fs_paths` | path to node / fs path | No: `get` only | comment |
| `orchestrator.rs` `crate_cache` | crate info memo | No: `get` only | comment |
| `orchestrator.rs` `loaded`, `edges` (link pass) | membership sets | No: `insert` only; edges are added in document order | comment |
| `orchestrator.rs` `symbol_lookup` | (path, name) to first node | No: `get` only; "first" is by node order | comment |
| `toon.rs` `dir_counts` | blast summary directory counts | **Yes**: sorted by count only, so equal counts followed hash order | `BTreeMap` and sort by (count desc, directory) |
| `query_engine.rs` `dist` (plan walk, blast walk) | BFS distance | No: drained into a vector sorted by a total order ending in the node index | comment |
| `query_engine.rs` `file_cache` | file lines | No: `get` only | comment |
| `doc_queries.rs` `merged`, `seen` | term merge; section dedup | No: result sorted by (count desc, term), terms unique; `seen` is `insert` only on a sorted list | comment |
| `docs.rs` `aggregate_terms` inner map | term to occurrences | No: sorted by (count desc, term) | `BTreeMap` (hardening) |
| `docs.rs` `Collector.seen`, `Slugger.occurrences`, `ResolveTables` maps | dedup, slug counts, lookup tables | No: membership or `get`; candidate lists keep insertion order, which is node order | comments |
| `results.rs` `QualifiedNamer.by_path` | symbols per path | No: `get` only; each list sorted | comment |
| `typed_queries.rs` `sources`, `parser.rs` `queries` | caches | No: `get` only | comment |
| `rust_use.rs`, tests | test helpers | n/a | none |

No unordered iteration is left on a path to the graph, the side tables or tool output. `read_dir` is used only in `get_active_features` (sorted) and in backup housekeeping.

## Tests
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` clean. `cargo test`: 359 passed, 0 failed (339 in 0.4.0; +12 resolver unit tests, +7 in `import_resolution_test.rs`, +1 in `determinism_test.rs`).

## Changed tests and goldens
- **No JSON golden file changed**; no field added, removed or renamed.
- `tests/regression_test.rs::test_regression_d7_file_outline_node_identity_and_imports` indexed all of `tests/fixtures`, where `module_b` exists as both `lua/module_b.lua` and `python/module_b.py`. 0.4.0 linked whichever came first in hash order (so the test passed by luck of the pick); the import is a true tie and is now left unlinked. The test now indexes `tests/fixtures/python` only, which keeps its intent (imported files are labelled `Import`).

## Deviations from the Specification
1. Beyond the plan: `toon.rs` blast summary tie order (found by the audit; fixed and tested).
2. `PROJECT_MAP_LOG=debug` was added to `build`/`refresh` so the spec's "debug level" log can be seen from the CLI. The MCP server keeps its own logging setup.
3. The ambiguous count is not a JSON field (as specified); it is only in the debug log.
4. The tier-2 direction deviation from Phase 1 stands.
