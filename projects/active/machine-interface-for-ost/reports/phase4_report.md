# Phase 4 Report: JSON, MCP and CLI Surface, Server Mode (machine-interface-for-ost)

Date: 2026-10-22. Gate task: 4.6. Next: Phase 5, after the owner's approval.

## Built

| Task | Result |
|:--|:--|
| 4.1 JSON renderer | `src/core/json_out.rs`: `to_json(tool, &Result<T, QueryError>, &IndexInfo)` wraps a typed result in the envelope (`tool`, `contract: 1`, `index`, `truncated`, `total`, `summary`, `results`; an error has `error {code, message}` and no `summary` or `results`). A `ToPayload` impl per result type places each field as the Specification's Data Models table says (`pick` keeps the summary to the named fields). JSON is never cut by characters; `doc_section` text is whole unless `max_chars` is given. `index.state` is `ok`, `stale` or `missing`, with the other `index` fields `null` when missing. JSON default item limits: 50 (`search`, `query`), 200 (`blast_radius`, `plan`); counts always cover every match. |
| 4.1 shared dispatcher | `src/core/tools.rs` (new, not in the Specification's component list): `ToolCall` (the 14 query tools), `Format` (`toon`/`json`, a clap `ValueEnum`), `ToolEnv`, `run(engine, env, call, format) -> ToolOutput { text, json, is_error }`. The MCP server and the CLI both call it, so the two surfaces cannot drift apart. TOON for the existing tools is the 0.3.0 formatter path; JSON is the typed result through `json_out`. |
| 4.2 MCP | `format` on all 15 tools; `limit` on `search`, `query`, `blast_radius`, `plan`; `max_depth` on `status` and `doc_outline`; new tools `modules` and `imports` (descriptions use the Specification's wording). Invalid `format` is `invalid_argument` with `is_error`. JSON returns the envelope as text content and the same object as `structured_content`; no `outputSchema`. `init` returns `status`, `message`, `commit`, `dirty`. The server sends `instructions` (about 1,300 characters; `server_details()` builds the initialize result, so a test can read it). New TOON forms for `modules` and `imports`. |
| 4.3 CLI | `--format toon\|json` on `find`, `context`, `impact`, `status`, `fetch`, `blast`, `search`, `doc`, `api` and the new `modules` and `imports`; `--limit` on `find`, `search`, `blast`, `impact`; `--max-depth` on `status` and `doc outline`. JSON goes to stdout only; a JSON error prints the error envelope to stdout and exits 1 (TOON errors still print and exit 0). `build`, `refresh`, `watch` and `mcp` reject `--format`. |
| 4.4 Goldens | `tests/fixtures/json_contract/`: 37 envelopes (every tool, plus limited, truncated, file-fallback, not-found, error and missing-index cases) with a README that makes regeneration (`UPDATE_GOLDENS=1`) a breaking-change step. Determinism test (3.6) extended: every CLI call now runs in TOON and in JSON across two differently named directories and across a rebuild. |
| 4.5 Server mode | Tracing subscriber `with_ansi(stderr().is_terminal())`. There was no progress output anywhere to remove (no progress-bar crate; the only stderr writes are the Git warning and the parse error line). Spawned-binary tests: see Tests. |
| File-level blast seeding | Approved by the owner this session. For a `matched: file` target, `blast_walk` also starts from every non-doc node of the file at cost 0. `blast -p src/core/query_engine.rs -s nope` on this repository: 26 dependents (25 direct), 26 files, `rust`; in Phase 3 it listed none. |

## Tests

- Before: 296 passing. After: **334 passing, 0 failed** (15 doctests ignored, as before). `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
- New: `json_out` unit tests (5), `json_out_test.rs` (6: every tool's envelope on the docs and imports fixtures, errors, the 50 and 200 limits), `mcp_format_test.rs` (11), `cli_format_test.rs` (8), `json_contract_test.rs` (3), `server_mode_test.rs` (4), `blast_test.rs` (+1: file seeding).
- `mcp_format_test.rs`: instructions are set and name every tool; every tool in both formats with `structured_content` equal to the parsed text; invalid `format`; error envelopes and `is_error` (TOON errors too); a missing index gives `index_missing` from every tool but `status`; a schema-3 index gives `index_stale` from `imports` and the doc tools; TOON `limit`; `max_depth`; `init` reports the commit; every tool declares `format` ("programs", "more tokens") and no `outputSchema`.
- `server_mode_test.rs` (spawned binary, piped stdio): in an empty directory `initialize` plus `status` answer in under 2 s and say the index is missing; `search` returns `index_missing` with `isError`; every stdout line is a JSON-RPC frame; stderr has no `\x1b[`; without `-w` the process holds no inotify descriptor and an edit leaves the index file's mtime and the `latest` link unchanged. Controls: with `-w` an inotify descriptor exists and the watcher logs, and that test fails if `with_ansi(true)` is put back (checked).
- Contract check: editing one golden (`direct` and `depth`) fails the test with a line diff.
- **0.3.0 tests changed** (all in `tests/mcp_test.rs`; the rest of the file passes unchanged):
  1. `test_mcp_tool_list_names`: the list gains `modules` and `imports`.
  2. `test_mcp_every_tool_description_names_its_use_case`: 13 becomes 15 tools, with a phrase for each new tool.
  3. `test_mcp_existing_tool_schemas_are_unchanged`: it compared whole schemas. It now requires every 0.3.0 property and `required` list to be unchanged and allows only the new optional `format`, `limit` and `max_depth`.
  4. `DummyRuntime` moved to `tests/common/mod.rs` so the new MCP tests share it.
  5. `determinism_test.rs` was extended, not changed (3.6 asked for it).

## Deviations from the Specification, and why

1. **`core/tools.rs` is a new module.** The Specification puts the surface in `server.rs`, `commands.rs` and `main.rs`. One dispatcher keeps TOON and JSON, MCP and CLI identical, and the CLI's JSON is tested against the MCP's.
2. **Stale index.** 4.2 says a missing or stale index gives the typed errors from every tool except `status`; the Technical Constraints say a schema-3 index keeps code and doc tools working and `imports` returns `index_stale`. I followed the more specific text: a missing index is `index_missing` everywhere; a stale one gives `index_stale` where data is absent (`imports`, and the doc tools, which already did) and code tools keep answering. `status` reports `stale`.
3. **TOON `limit`.** The Specification says an omitted TOON `limit` behaves as in 0.3.0 and says nothing about a given one. A given `limit` cuts the list and appends `[limit N: M matched]`, so the total is not lost. For `blast_radius` TOON the summary now prints the walk's total instead of the listed count (identical without a limit).
4. **`is_error` on TOON errors.** 0.3.0 returned error text without `isError`. Every error now sets it, in both formats; the text is unchanged. A program that read the text of a failing call still can.
5. **`fetch_symbol` TOON** now goes through `fetch_result`: the file is read relative to the index root rather than the server's working directory, and the old fallback to a line-based snippet when the byte range was out of bounds is gone (it reports `Error: Byte range or line out of bounds`). The messages for a missing symbol or an unreadable file are unchanged.
6. **TOON status and `doc_outline` cut notes.** When `max_depth` is given and cuts something, TOON appends `[tree cut at depth N]` or `[outline cut at depth N]`. Without `max_depth` the output is the 0.3.0 text.
7. **CLI details.** `doc section --max-chars` has no default in clap (TOON still uses 4000; JSON returns the full text unless given). `find` with neither `--query` nor `--file` keeps its old TOON message.
8. **JSON key order is alphabetical** (`serde_json` without `preserve_order`). It is deterministic, and a JSON object's key order carries no meaning, so I did not add the dependency; the goldens are sorted the same way.
9. **Version is still 0.3.0.** The bump belongs to the release (Phase 5/6); goldens replace `tool_version` with a placeholder so the bump will not touch them.

## Open items for the owner

1. **File nodes in a blast radius have `line: 0`, `end_line: 0` and a dotted name** (`rust_crate.src.user`), because that is how the index stores a file node. The Specification says lines are 1-based. The contract is pinned as it is; if P5/P8 should not see `0`, the options are to omit `line`/`end_line` for file nodes (a contract change, cheaper now than later) or to leave it and document it in 5.1. I did not change it unasked.
2. **A method target still finds nothing.** The file-level seeding fixes `matched: file`. `blast -s load` on `query_engine.rs` finds the symbol `load` (a method), which no `use` declaration names, so it reports no dependents. `blast -s QueryEngine` (the type) gives 26. If P8 will ask about methods, the walk could start from the enclosing type as well; not done.
3. Still open from before: apply `alwaysLoad` (`reports/tool-deferral.md`); the OST-Manager ROADMAP P5 gate line; `~/.cargo/bin` and the running `mcp -w` servers are 0.3.0. Nothing in OST-Manager was touched.

## Carried forward to Phase 5

README and CHANGELOG as in 5.1, plus: the TOON `limit` note and cut notes (deviations 3 and 6), `is_error` on errors (4), the file-seeding rule for `matched: file`, and the stale-index behaviour per tool (2).
