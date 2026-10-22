# Phase 1 report

## Built
- 1.1: `machine-interface-for-ost` task 6.4 marked complete; feature moved to `projects/completed/2026-10-22_machine-interface-for-ost` (git mv; the spec CLI has no archive command in this session, so the move was done by hand). `projects/active/` holds only this feature. No links into the old reports needed fixing.
- 1.2: 0.4.0 baseline, `reports/baseline.md`, `measure.py`, `baseline_0.4.0.json`. The defect reproduces: OST-Manager `config.py::Settings` 115 or 143, `/opt/wde` `logging.py` 171 to 176, with identical edge counts.
- 1.3: `tests/fixtures/ambiguous_imports/` with README and expected edges (tier 1 nearest, tie, `redis`/`myredis`, tier 2, tier 3).
- 1.4: `src/core/import_resolve.rs` with 12 unit tests; not yet wired in.

## Tests
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` clean; `cargo test` 351 passed, 0 failed (was 339; +12 unit tests).

## Deviations from the Specification
1. **Tier 2 direction.** The Specification says "the import ends with the FQN's full segment sequence" but its example (import `pkg.mod.Class`, FQN `pkg.mod`) and task 1.3 (`pkg.mod.Thing`, `pkg/mod.py`) describe the FQN as a *prefix* of the import. An import ends with an FQN only when a deep module name is shortened (`a.b.config` to `config`), which is a weak guess. Implemented as a prefix match (a member imported from a module), which is the stated intent.
2. **Empty segments.** An import with an empty segment (`.foo`, `foo.`) or an empty string never matches. Relative Python imports are not a suffix problem.
3. 0.4.0 timing is the median of 5 builds, not 3.
