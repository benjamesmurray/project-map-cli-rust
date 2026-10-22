# deterministic-import-resolution - Specification

Make import resolution deterministic, so that building the same source tree always gives the same graph and every tool gives the same output. 0.4.0 promised this (OST-Manager brief item 9) but does not deliver it when an import has more than one suffix match. This is a patch release, **0.4.1**: no new tools, no schema change and no JSON contract change.

**Target version**: `0.4.0` → `0.4.1`.

**Requested by**: OST-Manager. Verification of 0.4.0 on 2026-10-22 found the defect, and Wave A's close-out waits on this release. OST-Manager's P5 needs recorded runs to replay byte for byte, and its P8 normalises blast-radius counts, so both depend on it.

## Prerequisites

- **0.4.0 is released**: tag `v0.4.0`, crates.io and installed. Met 2026-10-22.
- **The previous feature is closed.** `machine-interface-for-ost` has one task left (6.4, publish, install and archive), which is done apart from archiving.
  - Mark 6.4 complete and archive the feature through the spec CLI, so `projects/active/` holds only this feature.
  - Record the result in `.epoch-context.md`.

## Background

**The defect.** Two builds of the same tree gave different `blast_radius` results:
- one copy of OST-Manager's `src/`, `docs/` and `tests/` gave 141 dependents of `src/ost_manager/config.py::Settings`, and a second copy gave 115;
- five rebuilds of one copy gave 141, 141, 141, 115 and 141.

The edge count was 4,431 every time, so the edges point at different nodes from build to build.

**Cause** (`src/core/orchestrator.rs`, pass 2, about line 297):
- Strategy 3, the fallback "FQN suffix match", runs `fqn_to_node.keys().find(...)` over a `std::collections::HashMap`.
- `HashMap` uses a randomly seeded hasher per map, so iteration order changes on every build, even within one process.
- When several file FQNs satisfy the predicate, the first one in that random order wins.

**Strategy 3's predicate is loose.** It matches when any of these is true:
- `k.ends_with(imp)`, as raw strings with no dot boundary, so `myredis` would match `redis`;
- `imp.ends_with(k)`;
- both have dots and their last two segments are equal.

That makes multiple matches common on Python projects whose packages live under `src/` (file FQN `src.ost_manager.x`, import `ost_manager.x`).

**Why the 0.4.0 determinism test passed.** `tests/determinism_test.rs` builds the docs and imports fixtures in two directories and compares output. Neither fixture has an import with more than one suffix match.

**A second, smaller source of variation.** Pass 1 walks files with `WalkBuilder::new(root)` and no sort (line 108). The walk follows directory-entry order, which depends on the filesystem, so node indices differ between machines. Query output is unaffected, because queries sort explicitly since 0.4.0. The index file, however, is not byte-stable across machines.

## Core Features & Scope

### In-Scope

1. **Deterministic, ranked Strategy 3.** Pass 2's fallback gathers **all** candidate file FQNs instead of the first one found, ranks them, and links only a unique best.
   - **Tiers**, best first. All comparisons are on whole dot-separated segments, never raw string suffixes:
     1. The FQN ends with the import's full segment sequence (`src.ost_manager.config` for `ost_manager.config`).
     2. The import ends with the FQN's full segment sequence (import `pkg.mod.Class`, FQN `pkg.mod`).
     3. Both have at least two segments, and their last two segments are equal.
   - **Within a tier**, a candidate with fewer extra leading segments wins (`src.ost_manager.config` beats `tests.fixtures.src.ost_manager.config`).
   - **Ties**: if two or more candidates are still tied, the import is left **unlinked**. Its `ImportEntry.resolved` stays `false`, and it is counted as ambiguous (item 3). Taking the lexicographically first would hide a guess, and loud failure is preferred.
   - Strategy 1 (exact FQN) and Strategy 2 (relative path) keep priority and are unchanged. The Rust `use` resolver (`rust_use.rs`) is unchanged; it already searches in a fixed order.
2. **No order-dependent lookups in the build.** Audit every `HashMap` and `HashSet` in `src/` whose iteration order can reach graph construction, a side table, or tool output, and give each one a total order. The options are `BTreeMap`/`BTreeSet`, sorting before iteration, or proving that the order is irrelevant (for example a lookup-only map), with the proof written as a code comment. The audit table goes in the phase report.
3. **Ambiguous imports are visible.**
   - The build counts imports left unlinked by a Strategy 3 tie and logs the first 20 at `debug` level, with each import string and its candidates.
   - The total goes into `reports/verification.md` for this repository, OST-Manager and `/opt/wde`.
   - No new `status` or JSON field: the JSON contract stays at 1. Callers already see `resolved: false` on those entries through `imports`.
4. **A sorted file walk.** Pass 1 sorts directory entries by file name (`WalkBuilder::sort_by_file_name`, or sorting the collected paths before parsing). Node and edge order in the index then depend only on the tree, so two builds of the same tree on any machine give a byte-identical index apart from `IndexMeta`'s build fields: `built_at`, `builder_pid`, `root`, and the Git fields when the trees differ.
5. **Determinism tests that would have caught this.**
   - A new fixture tree, `tests/fixtures/ambiguous_imports/`, has Python packages under `src/` and under a second root:
     - an import with one best candidate under the tier rules;
     - an import with an exact tie;
     - an import that 0.4.0's raw-string suffix rule mismatched (`redis` against a module `myredis`).
   - Tests:
     - each tier, each tie case and each non-match is resolved as specified;
     - **one fixture built 20 times in one process** gives an identical graph each time (the edge list as sorted `(from path, from symbol, to path, to symbol, type)` tuples, plus the import side table);
     - two copies in different directories give a byte-identical index after the build fields are removed;
     - the existing `determinism_test` cases also run on this fixture.
6. **Measured effect on real repositories.** On this repository, OST-Manager (at its current `HEAD`) and `/opt/wde`, record for 0.4.0 and 0.4.1:
   - edge count, `Imports` edge count, resolved and ambiguous import counts;
   - the `blast_radius` totals for a fixed list of five symbols per repository.

   For 0.4.0, report the spread over 5 builds. For 0.4.1, show that 5 builds agree exactly.
7. **Documentation.**
   - README: how imports are resolved (strategies 1–3 and the tie rule) and the determinism guarantee.
   - README: a note on `imports` package names. A Python package is named by its first segment, so `imports google` finds `google.cloud` imports and `imports "google.*"` does not. The prefix form is for scoped JS packages (`@sentry/*`).
   - CHANGELOG 0.4.1, listing the edge changes as a behaviour change under "Fixed".

### Out-of-Scope

- New tools, new JSON fields or a JSON contract bump. `ImportEntry.resolved` already carries the outcome.
- A schema version bump. The index shape is unchanged.
- Resolving Python imports through `sys.path`, `pyproject.toml` package roots or installed packages. Making Python resolution aware of `src/` layouts is a later feature, if the measurements in item 6 show many ambiguous imports.
- Call edges (`blast_radius` stays "dependents", as in 0.4.0).
- Making the `imports` prefix form match Python sub-packages: only a README note this release.

## Technical Constraints

- **Language/Runtime**: Rust 2021, existing crate. No new dependencies.
- **Data storage**: index JSON under `.project-map/`, schema 4, unchanged shape.
- **Key constraints**:
  - **Determinism is the release gate.** Every test in item 5 passes, and the item 6 measurements show identical results across 5 builds on all three repositories.
  - Indexing time on this repository and `/opt/wde` changes by no more than +5 % over 0.4.0 (median of 3, release build, measured as in `machine-interface-for-ost/reports/verification.md`).
  - The JSON contract golden files may change only in values that come from import edges (blast-radius members and counts). Every changed golden is listed in the phase report with the reason, and no field is added, removed or renamed.
  - Linking fewer edges is acceptable. Linking a wrong edge is not. Any tier rule that the measurements show linking implausible targets is tightened before release.

## User Stories

- As OST-Manager's run recorder (P5), I want the same tree to give the same graph, so that a recorded run replays byte for byte.
  - [ ] 20 builds of `tests/fixtures/ambiguous_imports/` in one process give identical sorted edge lists and import tables.
  - [ ] 5 builds of OST-Manager give identical `blast_radius` totals for the five fixed symbols.
- As P8's code-risk factor, I want blast-radius counts that do not wobble, so that risk scores are comparable between runs.
  - [ ] Two copies of OST-Manager in different directories give the same `blast_radius` JSON for `src/ost_manager/config.py::Settings`.
- As a coding agent, I want dependents that are real, so that I trust `blast_radius`.
  - [ ] An import of `redis` never links to a module named `myredis`.
  - [ ] A tied import is left unlinked and shows `resolved: false` in `imports`.
- As anyone comparing indexes, I want byte-stable index files, so that diffs show real changes.
  - [ ] Two builds of the same tree in two directories give identical index files once `IndexMeta`'s build fields are removed.

## Overview & Architecture

The change is confined to the build.
1. **Pass 1** collects paths in sorted order before parsing.
2. **Pass 2's Strategy 3** moves into a pure function in a new module, `src/core/import_resolve.rs`. It is pure (no I/O and no global state), so it is unit-testable without building an index. It takes the import string and the set of known file FQNs, and returns a resolution:
   - `Unique(fqn)`: one best candidate, linked;
   - `Ambiguous(candidates)`: a tie, left unlinked and counted;
   - `None`: no candidate.
3. The FQN table is a `BTreeMap`, or a sorted `Vec`, so candidate gathering is ordered.
4. Pass 2 also keeps the ambiguous count and the debug log.

No query code changes, because queries already sort since 0.4.0.

## Technical Stack

- **Backend**: Rust and the existing crates (`ignore`'s `WalkBuilder`, petgraph, serde).
- **Frontend**: none.
- **Utilities**:
  - the existing `tempfile` and determinism test helpers (`tests/determinism_test.rs`: `copy_fixture`, `built_in`);
  - a new `tests/fixtures/ambiguous_imports/` tree with a README listing which file exercises which rule;
  - a small measurement script or documented command list for item 6, kept in the report.

## Components and Interfaces

- `src/core/import_resolve.rs` (new):
  - `pub enum SuffixResolution { Unique(String), Ambiguous(Vec<String>), None }`;
  - `pub fn resolve_by_suffix(import: &str, fqns: &BTreeSet<String>) -> SuffixResolution`, with the tier and tie rules from item 1 and unit tests in the same file.
- `src/core/orchestrator.rs`:
  - a sorted walk in pass 1;
  - `fqn_to_node` ordered, or an ordered key set kept beside it;
  - Strategy 3 calls `resolve_by_suffix`;
  - the ambiguous count and debug log.
- `src/core/mod.rs`: register the module.
- `tests/import_resolution_test.rs` (new): the fixture-level resolution tests, the 20-build test and the byte-identical index test.
- `README.md`, `CHANGELOG.md`, `Cargo.toml` (0.4.1).

## Data Models

There are no persisted model changes. The function's types:

```rust
pub enum SuffixResolution {
    Unique(String),         // the single best file FQN; an Imports edge is added
    Ambiguous(Vec<String>), // tied best candidates, sorted; no edge, ImportEntry.resolved = false
    None,                   // no candidate; no edge, ImportEntry.resolved = false
}

/// Tiers, best first, all on whole dot-separated segments:
///   1. fqn ends with import's segments
///   2. import ends with fqn's segments
///   3. both have >= 2 segments and their last two segments are equal
/// Within a tier, fewer extra leading segments wins. A remaining tie is Ambiguous.
pub fn resolve_by_suffix(import: &str, fqns: &BTreeSet<String>) -> SuffixResolution;
```

## Verification Criteria

1. Unit tests for `resolve_by_suffix`:
   - each tier;
   - the leading-segment rule;
   - an exact tie returns `Ambiguous` with sorted candidates;
   - the raw-substring non-match (`redis` against `myredis`);
   - empty and single-segment imports.
2. Fixture tests on `tests/fixtures/ambiguous_imports/`:
   - expected edges;
   - `resolved` flags in `imports`;
   - the ambiguous count.
3. Determinism:
   - 20 in-process builds give identical graphs and import tables;
   - two directories give a byte-identical index once build fields are removed;
   - the existing `determinism_test` passes and now includes the new fixture.
4. Real repositories: the item 6 table in `reports/verification.md`, showing 0.4.0's spread and 0.4.1's exact agreement over 5 builds on this repository, OST-Manager and `/opt/wde`, plus the indexing-time check.
5. The `HashMap`/`HashSet` audit table is in the phase report, with no unordered iteration left on a path to the graph, side tables or output.
6. Every changed JSON golden is listed with its reason. The field set is unchanged (the contract tests' field checks pass without edits).
7. Release:
   - `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and the full `cargo test` are clean;
   - README and CHANGELOG are updated, and the version is 0.4.1;
   - after the owner's go-ahead: tag `v0.4.1`, publish to crates.io, `cargo install --version 0.4.1 --locked --force`, and restart running `mcp` servers followed by a `refresh` in each repository they serve;
   - a short hand-off note for OST-Manager.
