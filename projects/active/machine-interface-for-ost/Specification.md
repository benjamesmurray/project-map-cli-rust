# machine-interface-for-ost - Specification

Make project-map a dependable machine interface for OST-Manager's repo adapter while keeping it a compact, agent-first coding tool. This feature adds:
- opt-in JSON output with stable field names;
- the Git commit an index was built from;
- depth limits and module sizes;
- blast radius as numbers;
- a query for the files that import an external package;
- non-interactive server behaviour and deterministic ordering, both verified.

It completes OST-Manager's P4. Release 0.3.0 delivered only item 3 (GitHub anchors) and item 10 (crates.io) of the brief.

**Target version**: `0.3.0` → `0.4.0`. This is a minor release:
- index `schema_version` 3 → 4;
- two new MCP tools;
- a new optional `format` argument on every tool.

TOON output and existing tool names and parameters do not change.

**Requested by**: OST-Manager roadmap P4. The brief is `/home/benmurray/Projects/OST-Manager/projects/p4-project-map-uplift.md`, and 0.3.0's status against it is in `projects/completed/2026-10-22_doc-and-spec-indexing/reports/ost-manager-handoff.md`. OST-Manager's P5 (repo adapter), P7 (entropy monitor), P8 (code-risk factor) and P13 (growth scanner) consume this release. OST-Manager's ROADMAP will move P5's gate to "released in project-map ≥ 0.4.0".

## Prerequisites

- 0.3.0 is released and archived (`projects/completed/2026-10-22_doc-and-spec-indexing/`). Met 2026-10-22.
- No `project-map-cli-rust mcp` process older than 0.3.0 is running on this machine when the first 0.4 index is built. 0.3.0 and later have the `NewerIndex` forward guard, so a 0.3.0 watcher refuses to overwrite a schema-4 index; 0.2.x watchers do not have the guard. Check with `pgrep -af "project-map-cli-rust mcp"` and the binary each one runs. Record the result in `.epoch-context.md`.

## Background

What the code does today (0.3.0, `efd493a`):

- **Output is TOON text only.** `handle_call_tool_request` (`src/mcp/server.rs`) builds one string per tool and returns `CallToolResult::text_content`. The SDK's `CallToolResult` (rust-mcp-schema 0.10.3, protocol 2025-11-25) already has `structured_content`. The CLI subcommands print the same TOON.
- **`IndexMeta`** (`src/core/graph.rs`) records:
  - the schema and tool versions, root, build time and builder PID;
  - code and doc counts.

  It has no Git information. Nothing in the crate calls Git.
- **`status`** renders the module tree with a hard-coded depth of 3 (`render_tree(&paths, 3)`). `doc_outline` has no depth limit. Neither reports per-directory sizes.
- **`blast_radius`** (`QueryEngine::check_blast_radius`) is a stack-based walk over incoming edges. It returns a flat `Vec<NodeData>` in stack order, with no depth per result.
  - TOON already prints a `Summary:` block computed in `toon.rs`: total impacted nodes, unique files affected (**including the target's own file**) and affected modules.
  - The graph has **no `Calls` edges**. This repository's index has 673 `Contains`, 55 `References`, 11 `Imports` and 12 stream/config edges. The walk follows `Contains` upward, so a symbol's own file is listed as a "direct dependent". `blast` on `IndexMeta` (used across the crate) returns only `src/core/graph.rs`, because Rust `use` is not captured.
  - If the symbol is not found in the path, it silently falls back to the file's blast radius.
- **`search`** (`find_symbols`) returns matches in graph node order, with no ranking and no explicit sort. `analyze_impact` returns DFS order. Node order follows the file-walk and parse order of the build, so equal results are not guaranteed to come out in the same order across builds or machines.
- **Target selection**: `plan` takes the first node with a matching name anywhere in the repository; `blast_radius` and `fetch_symbol` take the first match within the path. Which one is "first" also follows node order.
- **Item limits**: `search`, `query`, `blast_radius` and `plan` have no `limit` argument. Their TOON is bounded only by the character budget.
- **Imports**:
  - Import strings are captured for Python, TypeScript/TSX, Kotlin, Lua, PHP, HCL and GDScript (`parser.rs` queries).
  - **Rust `use` declarations are not captured**, and imports carry **no line number**.
  - Imports that resolve to an indexed file or symbol become `Imports` edges (`orchestrator.rs`, pass 2). **Unresolved imports, which are exactly the external packages, are dropped.**
  - **`.js`, `.jsx`, `.mjs` and `.cjs` files are not indexed at all**, and CommonJS `require()` is not captured for TypeScript. That matters for OST-Manager P13 (`@sentry/node`, `stripe`, `posthog-js` in JavaScript repos).
  - Repositories commit build output: `/opt/wde` tracks a 1.7 MB Vite bundle (`viz-portal/frontend/dist/assets/index-*.js`) beside a 26 KB hand-written `app.js`.
- **Server**:
  - `mcp` runs over stdio and starts a watcher only with `-w`.
  - It loads the index if present and otherwise answers with `Error: Index not loaded`.
  - Its tracing subscriber is `fmt().with_writer(std::io::stderr)`, which emits ANSI colour codes by default.
  - It sends no server `instructions` (`instructions: None`). Claude Code shows server instructions to the model even when it defers the server's tools behind ToolSearch.
  - None of this behaviour has a test.

## Core Features & Scope

### In-Scope

1. **JSON output (brief item 1).**
   - Every MCP tool takes an optional `format` argument, `toon` (default) or `json`, and every CLI subcommand takes `--format toon|json`.
   - With `json`, the MCP result's text content holds the JSON document and `structured_content` holds the same object. Tools declare no `outputSchema`, because structured content is present only with `json`.
   - With `toon`, results have the same content as 0.3.0. The exceptions are list order (item 8, which reorders whole lists such as `search`, not only ties) and `blast_radius` (item 5).
   - The JSON envelope, field names and error codes are in **Data Models**. They are a public contract: a change to them is a breaking change, so it needs a major version bump (or, before 1.0, a minor bump with a CHANGELOG "Breaking" entry).
   - JSON is never cut by characters. Lists are capped by the tool's item limit, and the envelope says whether the list was truncated and how many matched (`truncated`, `total`). `doc_section` returns its full text in JSON unless `max_chars` is given, in which case `truncated` says so.
   - `search`, `query` (query form), `blast_radius` and `plan` gain an optional `limit`. In JSON it defaults to 50 for `search` and `query` and 200 for `blast_radius` and `plan`. In TOON, an omitted `limit` behaves as in 0.3.0 (character budget only). Summary counts always cover every match, not only the listed items.
   - The envelope's `index.state` is `ok`, `stale` or `missing`, so a program can tell a missing index from an empty result.
2. **Indexed commit (brief item 2).**
   - At build time the index records the Git commit of the root (`git rev-parse HEAD`), whether the working tree was dirty (`git status --porcelain` non-empty, untracked files included, ignored files excluded) and the branch, if any.
   - Git is called as a subprocess with `GIT_OPTIONAL_LOCKS=0`, so `git status` never takes `.git/index.lock` and cannot race the user's own Git commands.
   - At build time each call has a 5-second timeout. At query time (`status`) all calls share one 1.5-second budget, so `status` stays within 2 seconds on a large checkout; a field whose call ran out of time is `null`.
   - If the root is not in a Git repository, or Git is missing or times out, the fields are `null` and the build still succeeds. When Git ran but failed (for example "dubious ownership" in a container whose checkout belongs to another user), a warning goes to stderr and `status` reports the first line of Git's error as `git_error`.
   - `status` reports these fields, plus the current `HEAD` and dirty flag read at query time and an `at_head` boolean (indexed commit equals current `HEAD`, and neither is dirty).
   - A schema-3 index has no commit. It is reported as stale with `commit: null`.
3. **Depth limits (brief item 4).**
   - `doc_outline(path, max_depth?)` keeps headings at depth ≤ `max_depth`, where h1 is depth 1 and nesting follows the section tree, not the heading level (an h3 directly under an h1 is depth 2).
   - `status(max_depth?)` renders the module tree to that depth. The default is 3, as today.
   - Both report whether anything was cut (`truncated`). `doc_terms` keeps `limit`.
4. **Module sizes (brief item 5).** A new tool `modules(path_prefix?, max_depth=2)` and CLI subcommand `modules`.
   - For each directory under the prefix, down to `max_depth` levels below it, it lists:
     - `files`: indexed files;
     - `code_files`: files with a recognised language;
     - `symbols`: code symbols, excluding doc and API nodes;
     - `doc_files`;
     - `doc_sections`.
   - Each count is **recursive**: it includes subdirectories, including those below `max_depth`.
   - Directories are sorted by path. Files at the repository root are counted under the path `.`.
5. **Blast radius as numbers (brief item 6).**
   - **Meaning**: a dependent is a node that reaches the target through `Imports`, `Produces`/`Consumes` or `Configures` edges. The graph has no call edges, so `direct` counts direct *dependents* (importers, stream and config links), **not callers**. The README and the tool description say so, so P8 does not read it as a call count.
   - **Walk**: breadth-first over incoming edges, each node at its shortest distance (a 0-1 BFS).
     - Following `Contains` upward (from a symbol to its enclosing symbol and its file) costs depth 0. Those structural ancestors are part of the target, not dependents, and are never listed or counted. So a file that imports the target's file is depth 1, as is a file that imports the symbol itself.
     - Every other edge costs 1. Results carry `depth`: 1 for direct dependents, 2+ for transitive.
   - **Summary**:
     - `direct`: depth 1;
     - `transitive`: depth ≥ 2;
     - `total`;
     - `files_affected`: distinct paths of dependents, excluding the target's own file;
     - `languages`: the distinct languages of affected files, sorted;
     - `matched`: `symbol`, or `file` when the symbol was not found in the path and the file's blast radius was used (the 0.3.0 fallback). Neither found gives `not_found`.
   - Doc nodes stay excluded, as in 0.3.0.
   - **TOON**: the existing `Summary:` block gains the direct and transitive counts, and its "Unique Files Affected" uses the `files_affected` definition above. Per-node lines keep their format; the target's own file and enclosing symbols are no longer listed.
   - This only becomes useful on Rust once Rust `use` declarations resolve to `Imports` edges (item 6).
6. **Package-import query (brief item 7).** A new tool `imports(package, path_prefix?, limit=50)` and CLI subcommand `imports`.
   - **Records**:
     - The parser records every import with its 1-based line and the exact import line text (trimmed, at most 200 characters).
     - The orchestrator keeps every import in a side table, resolved or not, with a `resolved` flag. The flag is true when pass 2 linked it to an indexed file or symbol. Resolved imports still produce `Imports` edges as today.
   - **Package names**: each import gets a normalised package name:
     - Python: the first dotted segment (`sentry_sdk.integrations` → `sentry_sdk`);
     - JS/TS:
       - a scoped package keeps two segments (`@sentry/node/x` → `@sentry/node`);
       - an unscoped one keeps one (`stripe/lib` → `stripe`);
       - relative paths (`./`, `../`) and absolute paths have no package name;
     - Rust:
       - the first path segment of a `use` (`serde_json::Value` → `serde_json`);
       - `crate`, `self` and `super`, and the crate's own name (`project_map_cli_rust::…` in `tests/` and `main.rs`), have no package name;
       - `std`, `core` and `alloc` are reported as packages like any other crate;
     - PHP: the first namespace segment;
     - Kotlin: the full qualified name minus the last segment;
     - Lua: the `require` argument up to the first `.`;
     - HCL and GDScript: no package name (they import files).
   - **Matching**: the query matches a package name exactly. A query ending in `/*` or `.*` matches by prefix (`@sentry/*` matches `@sentry/node` and `@sentry/react`).
   - **Results**: path, line, the import text, the package name and `resolved`, sorted by path, then line, then module (`import a, b` gives two records on one line). The summary counts files and imports over every match, not only the `limit` listed.
   - **Coverage gaps closed for this query**:
     - **JavaScript** files (`.js`, `.jsx`, `.mjs`, `.cjs`) are indexed with the TSX grammar, including symbols, as in TypeScript.
     - `require("x")` and dynamic `import("x")` calls with a string literal are captured as imports in JS and TS.
     - Rust `use` declarations and `extern crate` are captured as imports, one record per statement (a grouped `use a::{b, c}` is one record).
     - **Rust resolution**: `crate::`, `self::`, `super::` and own-crate-name paths resolve in pass 2, so they become `Imports` edges and feed `blast_radius`. The crate root is the nearest ancestor directory with a `Cargo.toml`; `crate::a::b::Sym` resolves to the symbol `Sym` in `src/a/b.rs` or `src/a/b/mod.rs`, or to that file when the last segment is a module. A grouped `use` links each leaf. Edge counts for Rust fixtures grow accordingly; other languages' edge counts do not change.
     - **Built and minified JavaScript is skipped**: a `.js`/`.jsx`/`.mjs`/`.cjs` file is not indexed when it is named `*.min.*`, is larger than 256 KB, or has a line longer than 1,000 characters in its first 32 KB. TypeScript files are unaffected.
7. **Server mode (brief item 8).** These are made true where they are not already, and tested:
   - `mcp` without `-w` starts no watcher and never builds on its own.
   - `status` answers immediately when the index is missing (`index: missing`), stale, or at another schema, and says what to do (`refresh` or the `init` tool).
   - Other tools return a typed error (`index_missing`, `index_stale`), not a crash or a hang.
   - stderr carries no ANSI escape codes and no progress bars when it is not a TTY. The tracing subscriber sets `with_ansi(false)` unless stderr is a terminal.
   - stdout carries only JSON-RPC frames.
   - `init` is the in-band way to build when the server runs without `-w`. Its JSON summary includes the new index's `commit` and `dirty`, so a job does not need a second `status` call.
8. **Deterministic order (brief item 9).** Every list a tool returns has an explicit total order:
   - `search` and `query`: by path, then line, then name;
   - `blast_radius` and `plan`: by depth, then path, then line, then name;
   - `modules`: by path;
   - `imports`: by path, then line, then module;
   - the doc tools: their existing orders, made total with path, then byte offset as the final keys.
   - **Target selection is deterministic too**: when several nodes match the requested name (`plan` across the repository, `blast_radius` and `fetch_symbol` within a path), the first by path, then line, is used, and the result reports which one.

   Two builds of the same tree, from different directory names, give byte-identical output for every tool in both formats. Index-specific fields (`built_at`, `builder_pid`, absolute `root`) appear only in `status`.
9. **Agent ergonomics stay first.**
   - TOON stays the default and stays compact.
   - Each new tool's description says when to use it instead of grep, like the existing thirteen. `imports`: "which files use an external package, with the import line (grep cannot tell an import from a mention)". `modules`: "which areas of the codebase are largest, by files, symbols and doc sections".
   - The `format` argument's description says JSON is for programs and costs more tokens.
   - The server sends MCP `instructions`: a few lines saying what the index covers and to prefer the map tools over grep for symbols, dependents, imports and docs. These reach the model even when the client defers the tools.
10. **Tool deferral finding.**
    - Find out whether Claude Code loads the `map` MCP tools at session start or defers them behind ToolSearch, and whether a server or client setting changes that.
    - Evidence so far (2026-10-22 session): the `mcp__map__*` tools were listed as deferred, loadable through ToolSearch, while the server's `instructions` slot was empty.
    - Done in Phase 1, because the answer can shape the descriptions and server instructions built in Phase 4.
    - Record the answer, with sources, in the README's "Using with Claude Code" section.
    - This is a finding, not a code change. If a setting exists, the README says how to use it. The owner decides whether to apply it.

### Out-of-Scope

- Any LLM use, embeddings or semantic ranking (unchanged).
- New languages beyond JavaScript, and Java for decompiled APKs (parked in OST-Manager).
- Indexing website snapshots (OST-Manager's site adapter reads those itself).
- Resolving JS/TS bare imports through `node_modules` or `package.json`, or Python imports through installed packages. The package name comes from the import string only.
- Re-measuring map-tool use against grep in OST-Manager's transcripts. That happens in OST-Manager after the release, once item 10's finding is applied.
- Private-repository access, authentication, or any network call. Git is read locally only.

## Technical Constraints

- **Language/Runtime**: Rust 2021, existing crate.
  - Git through `std::process::Command` (no `git2` and no `libgit2`), with a timeout.
  - JSON through `serde_json`.
  - JavaScript through the existing `tree-sitter-typescript` TSX grammar, unless a fixture shows it failing on common JS, in which case `tree-sitter-javascript` is added. Decided in task 2.3 and recorded in the CHANGELOG.
- **Index compatibility**:
  - `schema_version` 4.
  - New `IndexMeta` fields (`git_commit`, `git_dirty`, `git_branch`) and the `imports` side table are `#[serde(default)]`.
  - A schema-3 index loads read-only and is reported as stale (the 0.3.0 mechanism, unchanged). Code and doc tools keep working on it. `imports` returns `index_stale`. `status` shows `commit: null`.
  - 0.3.0's forward guard means a 0.3.0 watcher refuses to overwrite a schema-4 index, and this is tested here.
- **Data storage**: the index JSON under `.project-map/` as today. The imports side table is bounded by the source: one record per import statement.
- **Key constraints**:
  - Indexing time on this repository and on `/opt/wde` grows by no more than 10 % over 0.3.0. The Git subprocess calls and import records are the new costs.
  - Index size grows by no more than 15 %.
  - Both are measured as in 0.3.0's `verification.md` (median of 3, release build).
  - No change to existing tool names or parameters. New optional parameters only: `format` (all tools), `limit` (`search`, `query`, `blast_radius`, `plan`) and `max_depth` (`status`, `doc_outline`).
  - No change to default TOON content beyond list order and `blast_radius` (item 5). Rust `context` output may list new import entries now that `use` resolves.
  - No network access.

## User Stories

- As OST-Manager's repo adapter (P5), I want JSON with stable field names, so that I turn results into citations without parsing TOON.
  - [ ] Every tool with `format: "json"` returns the envelope in Data Models. `structured_content` equals the parsed text content.
  - [ ] Golden contract files under `tests/fixtures/json_contract/` pin each tool's JSON on the docs fixture tree. A test fails on any added, removed or renamed field.
- As the repo adapter, I want to know which commit an index describes, so that I refuse to cite from an index that is not at the run's pinned SHA.
  - [ ] On a clean fixture Git repository, `status` JSON shows `commit` equal to `git rev-parse HEAD`, `dirty: false` and `at_head: true`.
  - [ ] After an edit, `dirty` is true at query time and `at_head` is false.
  - [ ] After a new commit without a rebuild, `at_head` is false.
  - [ ] Outside a Git repository, all Git fields are null and the build succeeds.
- As the Casting Director, I want depth-limited outlines and trees, so that a digest of at most 12,000 characters is cheap to build.
  - [ ] `doc_outline(max_depth=1)` and `status(max_depth=1)` return only the top level and report truncation.
- As P7's entropy monitor, I want module sizes, so that I send the Explorer to the largest area no citation has touched.
  - [ ] `modules(max_depth=1)` on the fixture tree gives recursive counts that sum correctly to the root's.
- As P8's code-risk factor, I want blast radius as counts, so that I normalise risk across repositories.
  - [ ] `blast_radius` JSON has `direct`, `transitive`, `total`, `files_affected`, `languages` and `matched`, and each result has `depth`.
  - [ ] On a Rust fixture crate, `blast_radius` on a struct used through `use crate::…` lists the importing files at depth 1, and the target's own file is not listed.
- As P13's growth scanner, I want the files that import a package, so that I find analytics, payment and monitoring SDKs without walking a checkout.
  - [ ] `imports("@sentry/*")`, `imports("stripe")`, `imports("sentry_sdk")` and `imports("serde_json")` each return the fixture's import lines with path and line, across JS, TS, Python and Rust.
- As an OST-Manager Cloud Run job, I want a quiet, non-interactive server, so that logs stay clean and a missing index is a clear error.
  - [ ] A spawned `mcp` with piped stdio answers `status` within 2 seconds on an empty directory, saying the index is missing.
  - [ ] Its stderr has no `\x1b[` bytes.
  - [ ] No file watcher thread runs without `-w`.
- As anyone replaying a recorded run, I want byte-stable output, so that recordings replay exactly.
  - [ ] Two builds of the fixture tree copied to two different directories give identical output for every tool in both formats, except `status`'s build fields.
- As a coding agent, I want nothing to get worse.
  - [ ] Every 0.3.0 MCP and CLI test still passes unchanged, except for tests that pin list order, the target's own file in `blast_radius`, or Rust import edges. Each such change is listed in the phase report.
  - [ ] Default TOON output sizes on this repository, for a fixed set of calls, are recorded before and after. None grows except `blast_radius` (its summary and its newly resolved Rust dependents) and Rust `context` imports.

## Overview & Architecture

The release has four layers, built in this order.

1. **Result models.**
   - Each tool's query function returns a typed result struct: `StatusResult`, `EntityList`, `BlastRadiusResult`, `ModulesResult`, `ImportsResult`, and the doc results.
   - Two renderers consume it: the existing TOON formatter (`toon.rs`) and a new JSON renderer (`json_out.rs`, via `serde`).
   - Today `doc_queries.rs` renders TOON inside `render_*` functions. Those split into a query part returning the struct and a rendering part, so both formats see the same data and the same ordering.
2. **Index additions (schema 4).**
   - At build start, `orchestrator.rs` reads Git state through `core/git.rs`.
   - The parser returns `ImportRecord { module, line, text }` instead of bare strings.
   - Pass 2 keeps all of them, with `resolved` and `package`, in the `imports` side table.
   - The parser also gains the JavaScript extensions and the new import captures.
3. **New queries.**
   - `modules` aggregates from file and symbol nodes and the doc side data.
   - `imports` filters the side table.
   - `blast_radius` becomes breadth-first and computes the summary.
   - The sorts from item 8 are applied in the query layer, not in the renderers.
4. **Surface.**
   - MCP: a `format` field on every tool struct, two new tools, and `structured_content` for JSON.
   - CLI: `--format`, `--max-depth`, and `modules` and `imports` subcommands.
   - The tracing subscriber is set up for non-TTY output.
   - README, CHANGELOG, version 0.4.0.

Errors become typed: `QueryError { code, message }`. TOON renders the message exactly as today, so existing error text is unchanged. JSON renders the code and the message.

## Technical Stack

- **Backend**:
  - existing: Rust, petgraph, tree-sitter grammars, `serde`/`serde_json`, `rust-mcp-sdk` 0.9 (protocol 2025-11-25, `CallToolResult.structured_content`), `tracing-subscriber`;
  - `std::io::IsTerminal` for TTY detection.
- **Frontend**: none.
- **Utilities**:
  - existing `tempfile` for fixture Git repositories, built in tests with `git init`, `git commit` and a fixed author and date;
  - golden JSON files under `tests/fixtures/json_contract/`;
  - a new `tests/fixtures/imports/` tree (JS, TS, Python, Rust, PHP files importing `@sentry/node`, `@sentry/react`, `stripe`, `posthog-js`, `sentry_sdk`, `serde_json`, plus relative and `crate::` imports that must have no package name).

## Components and Interfaces

- `src/core/git.rs` (new): `read_git_state(root: &Path, budget: Duration) -> GitState { commit: Option<String>, dirty: Option<bool>, branch: Option<String>, error: Option<String> }`, with `GIT_OPTIONAL_LOCKS=0`. Build time passes 5 seconds per call; query time passes one 1.5-second budget for all calls.
- `src/core/graph.rs`: `SCHEMA_VERSION = 4`, the `IndexMeta` Git fields, `imports: Vec<ImportEntry>` side table (`#[serde(default)]`).
- `src/core/parser.rs`:
  - `FileOutline.imports: Vec<ImportRecord>`;
  - `js`, `jsx`, `mjs` and `cjs` mapped to the TSX grammar, with built and minified files skipped;
  - new captures for `require`, dynamic `import()`, Rust `use_declaration` and `extern_crate_declaration`;
  - a per-language `package_name(language, module) -> Option<String>`.
- `src/core/orchestrator.rs`: Git state in meta; pass 2 records every import with `resolved`, and resolves Rust `crate::`/`self::`/`super::`/own-crate paths.
- `src/core/results.rs` (new): the result structs and `QueryError`, all `Serialize`.
- `src/core/query_engine.rs`:
  - `check_blast_radius` becomes a 0-1 breadth-first walk (`Contains` upward costs 0) with depths, a summary and `matched`;
  - deterministic target selection (path, then line) for `plan`, `blast_radius` and `fetch_symbol`;
  - `modules(prefix, max_depth)`;
  - `imports(package, prefix, limit)`;
  - explicit sorts.
- `src/core/doc_queries.rs`: `render_*` split into `*_result` queries plus rendering; `doc_outline` gains `max_depth`.
- `src/core/toon.rs`: renders the result structs. Output is unchanged except the `blast_radius` summary and listing (item 5) and list order.
- `src/core/json_out.rs` (new): `to_json(tool, result) -> serde_json::Value`, which wraps the envelope.
- `src/mcp/server.rs`:
  - `format` on all tool structs, `limit` on `search`, `query`, `blast_radius` and `plan`, with `modules` and `imports` tools added;
  - server `instructions`;
  - `structured_content` for JSON;
  - the typed missing and stale errors;
  - a non-TTY tracing subscriber;
  - descriptions for the new tools and argument.
- `src/cli/commands.rs`, `src/main.rs`: `--format`, `--max-depth`, `modules`, `imports`.

## Data Models

```rust
// Index additions (schema 4)
pub struct IndexMeta {
    // ... 0.3.0 fields unchanged ...
    #[serde(default)] pub git_commit: Option<String>, // full 40-hex SHA
    #[serde(default)] pub git_dirty: Option<bool>,
    #[serde(default)] pub git_branch: Option<String>, // None when detached
}
pub struct ImportEntry {
    pub path: String,            // repository-relative, forward slashes
    pub line: usize,             // 1-based
    pub text: String,            // trimmed import line, at most 200 chars
    pub module: String,          // as written: "@sentry/node", "sentry_sdk.integrations", "serde_json::Value"
    pub package: Option<String>, // normalised external package name, None for relative/crate-local/file imports
    pub language: String,
    pub resolved: bool,          // linked to an indexed file or symbol in pass 2
}
```

**JSON envelope** (every tool, `format: "json"`):

```json
{
  "tool": "blast_radius",
  "contract": 1,
  "index": { "state": "ok", "schema_version": 4, "commit": "<40-hex or null>", "dirty": false, "stale": false },
  "truncated": false,
  "total": 3,
  "summary": { "...": "tool-specific, see below; {} when none" },
  "results": [ { "...": "tool-specific items" } ]
}
```

`index.state` is `ok`, `stale` or `missing`. With `missing`, the other `index` fields are `null`.

On error the envelope has `"error": { "code": "...", "message": "..." }` and no `results`.
- The error codes are `index_missing`, `index_stale`, `not_found`, `invalid_argument` and `io_error`.
- MCP errors also set `is_error: true`.

**Item fields.** These are the stable names. A field that does not apply is omitted, never `null`, except the Git fields in `status`.

| Tool | Item fields | Summary fields |
|:--|:--|:--|
| `status` | (none; `results` is `[]`) | `next_step` (only when the index is missing), `tool_version`, `schema_version`, `root`, `built_at`, `commit`, `dirty`, `branch`, `head_now`, `dirty_now`, `at_head`, `git_error` (omitted when none), `stale`, `files`, `symbols`, `edges`, `doc_files`, `doc_sections`, `doc_references`, `api_operations`, `tree` (nested `{name, children}` to `max_depth`), `active_features` |
| `search`, `query` (query form) | `symbol`, `kind`, `node_type`, `path`, `line`, `end_line`, `role`, `preview` | — |
| `query` (path form) | `symbol`, `kind`, `line`, `end_line`, `role` and `path` (import entries only) | `path` |
| `query` (filename form) | `path` | — |
| `fetch_symbol` | — | `path`, `symbol`, `line`, `end_line`, `source` |
| `blast_radius` | `symbol`, `kind`, `node_type`, `path`, `line`, `end_line`, `depth` | `target_path`, `target_symbol` (omitted when `matched` is `file`), `target_line`, `matched`, `direct`, `transitive`, `total`, `files_affected`, `languages` |
| `plan` | `symbol`, `kind`, `node_type`, `path`, `line`, `depth` | `requested`, `target_symbol`, `target_path`, `target_line` |
| `doc_search` | `path`, `anchor`, `heading`, `level`, `line`, `score`, `snippet` | `query` |
| `doc_outline` | `anchor`, `heading`, `level`, `depth`, `line`, `end_line` | `path`, `frontmatter` |
| `doc_section` | — | `path`, `anchor`, `heading`, `line`, `end_line`, `text` |
| `doc_links` | `direction` (`to_docs` or `to_code`), `path`, `anchor`, `heading`, `target_path`, `target_symbol`, `form` | `path`, `symbol`, `total` |
| `doc_terms` | `term`, `count`, `sections` (`[{path, anchor}]`) | `path_prefix` |
| `api_surface` | `path`, `item` (`operation` or `schema`), `method`, `route`, `operation_id`, `name`, `summary`, `line` | `total` |
| `modules` | `path`, `depth`, `files`, `code_files`, `symbols`, `doc_files`, `doc_sections` | `path_prefix`, `max_depth` |
| `imports` | `path`, `line`, `text`, `module`, `package`, `language`, `resolved` | `query`, `files`, `imports` |
| `init` | — | `status` (`refreshed` or `failed`), `message`, `commit`, `dirty` |

Further rules:
- Every list result also reports `truncated` and `total` in the envelope (the number of matches before any limit). In `status` the envelope's `truncated` says whether the tree was cut at `max_depth`.
- In `blast_radius`, `direct`, `transitive`, `total`, `files_affected` and `languages` are always present (`0` / `[]` when nothing depends on the target), and every item has `depth`.
- `symbol` is qualified when the symbol lies inside another symbol's range in the same file (`Graph.load`); otherwise it is the bare name. A Rust `impl Trait for Type` block is named after `Type`. Qualified names are **not unique** (two `impl From<…> for E` blocks both give `E.from`), so a citation needs `path` and `line` as well.
- Lines are 1-based. `end_line` is the last line of the node's byte range.
- `contract` is the JSON contract version, which starts at 1. It is independent of `schema_version`.

## Verification Criteria

1. **Contract**:
   - golden JSON files for every tool on the docs fixture and imports fixture trees;
   - `structured_content` equals the text content for every MCP tool;
   - TOON output for every 0.3.0 MCP test is unchanged.
2. **Git**: the fixture Git repository cases in the user story (clean, dirty, new commit, no repository, Git missing via an empty `PATH`), no `.git/index.lock` taken (`GIT_OPTIONAL_LOCKS=0` set on every call), `git_error` on a failing Git, and the query-time budget.
3. **Depth and modules**: unit and MCP tests for `max_depth` on `doc_outline` and `status`, and recursive `modules` counts.
4. **Blast radius**: breadth-first depths on a diamond-shaped fixture (a node reachable at depths 1 and 3 is reported at depth 1), the summary counts, `Contains` ancestors never listed, an importer of the target's file at depth 1, the `matched: file` fallback, and a Rust fixture crate whose `use crate::…` importers appear.
5. **Imports**: every package-name rule, the prefix query, the no-package cases, and JS files indexed with their symbols. `resolved` is true for a relative import of an indexed file and for a `crate::` path. Minified and oversized JS files are skipped.
6. **Server mode**: a spawned-binary test covers:
   - `status` on an empty directory within 2 seconds;
   - typed errors from another tool;
   - no ANSI bytes on stderr;
   - no watcher without `-w`.
7. **Determinism**: a two-directory, two-build test over every tool in both formats, including ambiguous target names for `plan`, `blast_radius` and `fetch_symbol`.
8. **Compatibility**:
   - a schema-3 index loads read-only as stale, with `commit: null`;
   - building over a schema-5 index fails with `NewerIndex`;
   - a 0.3.0-shaped struct deserialises a schema-4 index.
9. **Performance**: indexing time and size on this repository and `/opt/wde` before and after, recorded in `reports/verification.md` against the constraints above, together with TOON output sizes for the fixed call set.
10. **Release**:
    - README (formats, new tools, the "Using with Claude Code" finding, upgrade notes) and CHANGELOG updated;
    - version 0.4.0;
    - fmt and clippy clean;
    - published to crates.io and installed globally after the owner's go-ahead;
    - a hand-off report for OST-Manager mapping each brief item to its evidence.

**Post-release, in OST-Manager, not a release gate**: P5's first task runs the 0.4.0 binary's JSON on a small fixture repository and on OST-Manager's `docs/`, and checks that every tool's results map to citations that resolve.
