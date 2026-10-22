# Phase 5 Report: README and CHANGELOG (machine-interface-for-ost)

Date: 2026-10-22. Gate task: 5.2. Next: Phase 6, after the owner's approval.

## Built

| Task | Result |
|:--|:--|
| 5.1 README | New sections: "Using with Claude Code" (the 1.5 deferral finding, the server instructions, the `alwaysLoad` / `ENABLE_TOOL_SEARCH` / per-tool `anthropic/alwaysLoad` table and an example entry for the owner to apply), "JSON Output" (envelope, error codes, field table for all 15 tools, contract versioning rule), "Status, Git State and Depth Limits" (`commit`, `dirty`, `branch`, `head_now`, `at_head`, `git_error`, `max_depth`), "Modules and Imports" (package-name rules per language, JavaScript indexing and skip rules, Rust `use` resolution), "Blast Radius Meaning" (dependents not callers, file seeding, `via`, TOON `limit` note), "Server Mode", "Deterministic Order" and "Upgrading to 0.4.0" (schema 4, restart servers, compatibility). The feature list, tool list and CLI usage gained `modules`, `imports`, `format`, `limit`, `max_depth`. CLI examples were run against the built binary. |
| 5.1 CHANGELOG | `[0.4.0] - Unreleased`: upgrade note, **Breaking** (blast_radius meaning, list order, `is_error` on every error, `fetch_symbol` path base), Added (JSON, `limit`, `max_depth`, Git state, `modules`, `imports`, JavaScript with the grammar choice and skip rules, Rust import resolution, file and method seeding, server instructions), Changed. The date is filled at release (6.3). |

## Tests

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` clean. `cargo test`: **339 passed, 0 failed**, 15 doctests ignored (as before). Phase 5 changed documentation only.
- `cargo package --list` does not include the nested `tests/fixtures/imports/rust_crate/Cargo.toml` (Cargo skips nested packages), which closes the Phase 1 open item.

## Deviations from the Specification, and why

1. The README has no section literally named "Using with Claude Code" before this task, so it is new. The Specification says to record the finding "in the README's 'Using with Claude Code' section".
2. The deferral finding is documented from Claude Code's documentation; `alwaysLoad` was not trialled in a fresh session (as in `tool-deferral.md`). The README says so.
3. The CHANGELOG lists the `fetch_symbol` path-base change and the `is_error` change under **Breaking**, because they alter what a caller can see, although the Specification only names `blast_radius` and list order.

## Open items for the owner

- Apply `alwaysLoad` if wanted (README shows the entry).
- `~/.cargo/bin` and the running `mcp -w` servers are still 0.3.0.
- Phase 6 not started: waiting for your go-ahead.
