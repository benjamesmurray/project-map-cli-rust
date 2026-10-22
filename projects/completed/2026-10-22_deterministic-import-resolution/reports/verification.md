# 0.4.1 verification (task 3.1)

Method as in `baseline.md`: `measure.py` run with the 0.4.0 binary (installed) and the 0.4.1 release build (`target/release`), 5 builds per repository into a scratch `--out`. Inputs: `self` is a `git archive` of `a516255`; OST-Manager at `a7946e9`; `/opt/wde` as on 2026-10-22. Raw data: `baseline_0.4.0.json` (0.4.0 first run), `timing_0.4.0.json` (0.4.0 re-run beside the 0.4.1 run, used for time), `verification_0.4.1.json`.

## Determinism

**Every number is identical across the 5 builds of 0.4.1 on all three repositories** (edges, `Imports` edges, resolved, unresolved, ambiguous, and all fifteen blast totals; `identical_across_builds: true` in the JSON). 0.4.0 varies on OST-Manager and `/opt/wde` (`baseline.md`).

Two directory copies of OST-Manager (`src/`, `docs/`, `tests/` in `alpha/` and `omega-longer-name/`), built with 0.4.1: `blast -p src/ost_manager/config.py -s Settings` gives byte-identical JSON (apart from the `index` block) and identical TOON; total 115.

## Edges and imports, 0.4.0 → 0.4.1

| Repository | Edges 0.4.0 | Edges 0.4.1 | `Imports` 0.4.0 | `Imports` 0.4.1 | Resolved 0.4.0 | Resolved 0.4.1 | Unresolved 0.4.0 | Unresolved 0.4.1 | Ambiguous (0.4.1) |
|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| self | 2452 | 2447 | 292 | 287 | 194 | 189 | 233 | 238 | 5 |
| OST-Manager | 8129 | 8100 | 657 | 628 | 657 | 628 | 482 | 511 | 0 |
| wde | 42678 | 42342 | 1942 | 1606 | 1942 | 1606 | 2961 | 3297 | 22 |

All 0.4.0 counts were identical across its 5 builds; only the targets varied. 0.4.1 links fewer imports, none more (`gained: 0` on OST-Manager and `/opt/wde`).

## `blast_radius` totals (five fixed symbols per repository)

| Repository | Symbol | 0.4.0 (distinct values over 5 builds) | 0.4.1 (all 5 builds) |
|:--|:--|:--|:--|
| self | `src/core/graph.rs::src.core.graph` | 19 | 19 |
| self | `src/core/mod.rs::src.core` | 21 | 21 |
| self | `src/core/orchestrator.rs::Orchestrator` | 18 | 18 |
| self | `src/core/query_engine.rs::QueryEngine` | 26 | 26 |
| self | `src/core/toon.rs::ToonFormatter` | 10 | 10 |
| OST-Manager | `src/ost_manager/config.py::Settings` | 115, 143 | 115 |
| OST-Manager | `workspace.py::src.ost_manager.contracts.workspace` | 64 | 58 |
| OST-Manager | `src/ost_manager/llm/client.py::src.ost_manager.llm.client` | 49 | 43 |
| OST-Manager | `src/ost_manager/ost/errors.py::src.ost_manager.ost.errors` | 98, 134 | 92 |
| OST-Manager | `src/ost_manager/ost/store.py::src.ost_manager.ost.store` | 83, 132 | 77 |
| wde | `Bar.kt::streams.wde-contracts.src.main.kotlin.com.wde.contracts.Bar` | 62 | 55 |
| wde | `PhaseLabelEvent.kt::streams.wde-contracts.src.main.kotlin.com.wde.contracts.PhaseLabelEvent` | 71 | 71 |
| wde | `optimizer.py::utils.physics-lab.optimizer.optimizer` | 65, 73, 78, 83 | 56 |
| wde | `viz-portal/backend/app/api.py::viz-portal.backend.app.api` | 24, 29 | 24 |
| wde | `logging.py::viz-portal.backend.app.logging` | 171, 174, 175, 176 | 183 |

Totals that fall (for example `ost/store.py` 83 or 132 → 77) fell because wrong links were dropped; `logging.py` rises (171 to 176 → 183) because a unique best target is now chosen where the random pick used to land elsewhere.

## Indexing time (median of 5 builds, release, 0.4.0 re-run beside 0.4.1)

| Repository | 0.4.0 (s) | 0.4.1 (s) | Change |
|:--|--:|--:|--:|
| self | 0.251 | 0.249 | -1 % |
| OST-Manager | 0.235 | 0.225 | -4 % |
| wde | 2.080 | 1.686 | -19 % |

Within the +5 % limit. A first, naive version of the resolver (splitting every FQN for every import) was +45 % on `/opt/wde`; `SuffixIndex` (FQNs indexed by first and last segment, compared to a brute-force reference in a unit test) fixed that.

## Link sample (20 newly unlinked, 20 still linked, per repository)

Script: a sampler over the two index files (random, seed 1); a listing of an import and the importer's edge targets with a matching name.

- **Newly unlinked.** OST-Manager 29: `re` 24, `redis` 4, `string` 1. `/opt/wde` 336: `os` 181, `time` 52, `math` 38, `re` 23, `yaml` 19, `ast` 5, `random` 5, `io` 4, `optuna` 4, `common` 3, `numba` 1, `types` 1. Apart from `common` (a bare name with several candidate files), these are standard-library or third-party packages that 0.4.0 linked to some project file whose name ends in the same letters (the raw-string rule). The 40 sampled were all of this kind; leaving them unlinked is correct.
- **Still linked.** All 40 sampled are correct: OST-Manager `ost_manager.x.y` to `src/ost_manager/x/y.py`; `/opt/wde` Kotlin `com.wde.…` classes to their files, Python `app.api`, `src.rpc.pool_abi`, `ape_core.alphas.…` to the one file ending in that module path. Three look cross-project but have a single candidate and are plausible `sys.path` imports (`validator.services` to `utils/env-reset-relaunch-utility/validator/services.py`, `optimizer.common` to `utils/physics-lab/optimizer/common.py`, `probes.objective_audit`). No tier rule needs tightening.
- **Ambiguous (left unlinked on purpose):** 0 on OST-Manager, 22 on `/opt/wde`, 5 on this repository (`module_a`/`module_b` that exist as both Lua and Python fixtures).

## Specification coverage

| Item | Task | Test or evidence |
|:--|:--|:--|
| 1 Ranked Strategy 3, tie unlinked | 1.4, 2.1 | `import_resolve` unit tests (13); `fixture_edges_match_the_readme`, `tied_and_unmatched_imports_…` |
| 2 No order-dependent lookups | 2.3 | audit table, `phase2_report.md`; `blast_summary_lists_equal_sized_directories_in_name_order` |
| 3 Ambiguous imports visible | 2.1 | `the_ambiguous_import_is_counted_in_the_debug_log`; counts above |
| 4 Sorted file walk | 2.2 | `two_directories_give_a_byte_identical_index` (ambiguous, imports and docs fixtures) |
| 5 Determinism tests | 1.3, 2.4 | fixture `ambiguous_imports`; `twenty_builds_in_one_process_…`; `determinism_test` (new fixture, both formats) |
| 6 Measured effect | 1.2, 3.1 | this file and `baseline.md` |
| 7 Documentation | 3.2 | README, CHANGELOG |

| Verification criterion | Result |
|:--|:--|
| 1 Unit tests | pass |
| 2 Fixture tests | pass |
| 3 Determinism (20 builds, two directories, existing test) | pass |
| 4 Real repositories | identical over 5 builds on all three; time -1 %, -4 %, -19 % |
| 5 Audit table | in `phase2_report.md` |
| 6 Goldens | none changed; contract field checks unchanged |
| 7 Release | after 3.2 and 3.3; publish waits for the owner |
