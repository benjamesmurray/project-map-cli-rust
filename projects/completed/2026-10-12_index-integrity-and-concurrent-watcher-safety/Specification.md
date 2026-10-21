# index-integrity-and-concurrent-watcher-safety - Specification

Prevent index wipes from concurrent watchers: correct corrupt-index recovery root, atomic index writes, single-writer lock, path normalisation, empty-index visibility.

**Target version**: `0.1.18` → `0.2.0` (minor bump: stored/returned paths change from `./a/b.py` to `a/b.py`; see §Data Models).
**Reported from**: `/opt/wde` workspace session, 2026-10-12 (reproduced live; evidence below).

---

## Background: Observed Failure & Root Cause

### Symptom
In `/opt/wde` (≈17,700 nodes, 6.5 MB index), with two or more `project-map-cli-rust mcp -w` processes running against the same root (one spawned by Claude Code, one or two by Antigravity), the index was intermittently replaced by a **6-node, 1.6 KB index** containing only:

```
.project-map/ARCHITECTURE.md
.project-map/backups/<ts>/.project-map.json   (x5)
```

After that, every `query`, `search` and `fetch_symbol` returned `Matches Found: 0`. `status` still reported `Phase: Ready / Index: Found`, so the failure was silent. Running the `init` tool restored the index.

### Reproduction (2026-10-12)
| Probe | Result |
|:--|:--|
| New `.py` in a gitignored dir (`temp/`) | Ignored — correct |
| New `.py` in `utils/`, then deleted | Merged / removed; index stays ~6.5 MB — correct |
| New `.json` in a tracked dir | Merged — correct |
| Byte-identical rewrite of a tracked 16 KB JSON file, 15:21:23 | **Index wiped to 1,647 bytes at 15:21:28** |
| Same rewrite, 15:23:57 | No wipe |

Because the same write sometimes wipes and sometimes doesn't, this is a **timing race between processes**, not a trigger tied to a particular file.

### Root causes
| # | Defect | Location |
|:--|:--|:--|
| D1 | **Corrupt-index recovery uses the wrong root.** On a JSON parse error, `Graph::load` sets the root to `path.parent().parent()`. For `.project-map/latest/.project-map.json` that is `.project-map`, not the project root. It then calls `scaffold_if_empty(".project-map")`, which writes the placeholder `.project-map/ARCHITECTURE.md`, then `build_index(".project-map")`, and saves the result over `latest`. The FQNs in the wiped index (`backups.20261012_150854..project-map.json`) confirm the root was `.project-map`. | `src/core/graph.rs` `Graph::load` (~L121–145) |
| D2 | **A reader writes.** Any load (MCP startup, watcher reload, `init`, CLI query) that hits a parse error rebuilds and **overwrites the shared index**. A transient read error is therefore escalated into persistent data loss. | `Graph::load` |
| D3 | **Index writes are not atomic.** `Graph::save` uses `fs::write`, which truncates and writes the 6.5 MB file in place. The `latest` pointer is updated as *remove, then create symlink*. A concurrent reader can see a half-written file (parse error → D1/D2) or no `latest` at all. | `Graph::save`, `Orchestrator::save_index_versioned` |
| D4 | **No coordination between processes.** Each watcher does a full rebuild on every relevant event. N watchers on one root mean N concurrent rebuilds and writes per change. Backup dir names have one-second resolution, so they collide. Rotation (`remove_dir_all` of the oldest) can delete a directory another process is writing or that `latest` points to. | `watcher.rs`, `save_index_versioned` |
| D5 | **Path lookup is exact-string.** Stored paths are `./utils/x.py` (root `.`), so `query path="utils/x.py"` silently returns "No symbols detected". | `QueryEngine::get_file_outline` (and the `path ==` matches at L86, 96, 125) |
| D6 | **Empty or degenerate indexes are invisible.** `status` reports `Ready / Found` with no counts, root, build time or sanity check. | `server.rs` `status`, `main.rs` status |
| D7 | *(To investigate)* The file outline for `./utils/physics-lab/optimizer/common.py` lists the file node as `ingestion.src.rpc.price_math`. Either a node-name or edge-target mix-up, or the outline includes import targets mislabelled as the file. | `get_file_outline` / FQN mapping |
| D8 | **Watcher and builder exclude the output dir by name only** (`.project-map`). A custom `--out` dir inside the root would be indexed and would retrigger the watcher. | `watcher.rs`, `orchestrator.rs` filters |

---

## Core Features & Scope

### In-Scope
1. **Fix the recovery root and make readers read-only (D1, D2).**
   - `Graph::load` / `QueryEngine::load` never build, scaffold or write. A parse failure returns a typed error (`AppError::CorruptIndex`).
   - Callers retry transient failures: up to 3 attempts with 100 ms / 250 ms / 500 ms backoff.
   - If the index is still unreadable: the **MCP server keeps the previous in-memory engine** and logs a warning, and **CLI commands** exit with an actionable error suggesting `project-map refresh`.
   - Rebuilding happens only through explicit build paths (`build`/`refresh`, MCP `init`, watcher), and those always receive the **project root explicitly**.
2. **Atomic index persistence (D3).**
   - Write the index to a temp file in the same directory (`.project-map.json.tmp.<pid>.<nanos>`), `fsync`, then `rename` over the target.
   - Update `latest` atomically: create a symlink at a temp name, then `rename` it over `latest` (POSIX). On platforms without symlinks, `latest/` becomes a directory updated with copy-to-temp + rename.
   - Backup directory names have millisecond resolution plus a pid suffix: `YYYYMMDD_HHMMSS_mmm_<pid>`.
3. **Single-writer coordination (D4).**
   - Hold an advisory exclusive lock on `<out>/.lock` (via `fd-lock` or `fs4`) around build, save and rotation.
   - On a watcher event, a process that gets the lock and finds `latest` newer than the triggering event *skips the rebuild and just reloads* (coalescing). A process that can't get the lock within a timeout (default 30 s) skips the cycle and logs; the next event retries.
   - Readers take no lock; atomic rename makes reads safe.
   - Rotation runs under the lock, keeps the newest 5, and **never deletes the directory `latest` points to**.
4. **Root and output invariants (D1, D8).**
   - `build_index(root, out_dir)` refuses (error) when `root` equals `out_dir` or is inside it.
   - `out_dir` is excluded **by canonical path**, in addition to the existing name filters, in both the builder walk and `watcher::is_ignored_path`.
   - `scaffold_if_empty` never writes inside `out_dir`.
5. **Path normalisation (D5).**
   - One helper, `normalize_rel_path(root, p) -> String`: strips a leading `./`, makes absolute paths relative to the root, and uses `/` separators.
   - Used when **indexing** (stored node paths), when **querying** (`query path=`, `fetch_symbol path=`, `blast_radius path=`, `plan`) and when **loading** older indexes. Old `./`-prefixed indexes are normalised in memory on load, so no rebuild is required.
6. **Index metadata and status visibility (D6).**
   - Add an index `meta` block: `schema_version`, `tool_version`, `root` (canonical absolute path), `built_at` (RFC 3339), `builder_pid`, `file_count`, `symbol_count`, `edge_count`.
   - `status` (CLI and MCP) reports these values.
   - `status` shows a **WARNING** when `file_count == 0`, or when every indexed path is under `out_dir`, or when `meta.root` ≠ the server's root.
7. **Investigate and fix D7**: add a failing test that reproduces the wrong file-node name in the outline, then fix it. If it turns out to be intended behaviour (import targets), label import targets distinctly in the outline output instead.
8. **Regression and concurrency tests** (see Verification).
9. **Release prep**:
   - Bump `Cargo.toml` to `0.2.0`.
   - Add `CHANGELOG.md` with a `0.2.0` entry covering the fixes and the path-format change.
   - Update the README sections on the watcher, concurrency and `status`.
   - `cargo package` / `cargo publish --dry-run` must succeed.
   - The user publishes to crates.io and pushes to the existing GitHub remote (`benjamesmurray/project-map-cli-rust`).

### Out-of-Scope
- Incremental, per-file re-indexing in place of full rebuilds (a performance follow-up; the lock and coalescing in this project bound the cost).
- A single shared daemon serving several MCP clients (an alternative to locking; possible follow-up).
- Changes to MCP tool names or argument schemas, except that path arguments now also accept non-prefixed paths.
- Changes to `deliver-cli` or to client configs (AGY / Claude Code).

---

## Technical Constraints
- **Language/Runtime**: Rust 2021; tokio runtime (existing).
- **Platforms**: Linux (primary; inotify), macOS. Windows must compile and degrade gracefully (copy + rename fallback for `latest`; the lock crate must support Windows).
- **Dependencies**: at most one new crate for file locking (`fd-lock` or `fs4`, both maintained and cross-platform). Add `tempfile` as a **dev-dependency** only (tests).
- **Backward compatibility**:
  - Indexes written by ≤ 0.1.18 (no `meta`, `./` paths) MUST load (`#[serde(default)]` on `meta`; paths normalised on load).
  - A 0.2.0 index MUST load in a 0.1.18 reader without a parse error (only additive JSON fields; `meta` is ignored by older serde structs unless they use `deny_unknown_fields` — verified 2026-10-12: `SerializableGraph` has no `deny_unknown_fields`).
  - Mixed 0.1.18 / 0.2.0 processes on one root are **not** made safe. The rollout procedure restarts all MCP clients (§Rollout).
- **Invariant (NORMATIVE)**: no code path that loads an index may write to `out_dir`.
- **Determinism**: index content for an unchanged tree is identical across rebuilds, apart from `meta.built_at` / `builder_pid`. Node order is stable.

---

## User Stories
- As an **agent using the map MCP server**, I want lookups to keep working while another agent's watcher rebuilds the index, so that I never silently get "0 matches" for code that exists.
  - [ ] Two watchers plus a reader under a 200-event write storm: the reader never sees a parse error or an index with fewer than 95% of the expected files.
  - [ ] A corrupt `latest` index never results in a write to `out_dir` by a reader.
- As an **agent**, I want `query path="utils/x.py"` and `query path="./utils/x.py"` to return the same outline.
  - [ ] Both forms (and the absolute path) resolve identically for `query`, `fetch_symbol`, `blast_radius` and `plan`.
- As an **operator**, I want `status` to tell me how big and how fresh the index is and to warn when it is degenerate, so that silent failures become visible.
  - [ ] `status` shows root, built_at, file/symbol/edge counts and tool version.
  - [ ] `status` prints a WARNING for an empty or out-dir-only index.
- As a **maintainer**, I want the release to install cleanly from crates.io and to upgrade existing installs without manual index surgery.
  - [ ] `cargo publish --dry-run` passes; an existing 0.1.18 index loads in 0.2.0.

---

## Overview & Architecture

```
            ┌────────────── writers (exclusive .lock) ──────────────┐
 watcher ─┐ │ acquire lock → (latest newer than event? reload only)  │
 init    ─┼─▶ build_index(ROOT, OUT) → Graph{meta,…}                 │
 refresh ─┘ │ save: tmp → fsync → rename ; backups/<ms>_<pid>/       │
            │ latest: tmp-symlink → rename ; rotate (keep 5, ≠latest)│
            └────────────────────────────────────────────────────────┘
 readers (no lock): QueryEngine::load(latest) → retry/backoff on CorruptIndex
                    MCP: keep previous engine on failure ; CLI: actionable error
                    paths normalised on load and on every query argument
```

* **Writers** are the only code that touches `out_dir`; they take the lock and write atomically.
* **Readers** are pure. They can at worst read the previous complete index, never a partial or foreign one.
* The project **root is passed explicitly** everywhere (`McpServer` / `ProjectWatcher` carry `root` and `out`). Nothing derives the root from the index path.

---

## Technical Stack
- **Core**: Rust 2021, `serde`/`serde_json`, `petgraph`, `ignore`, `notify`, `tokio`, `chrono` (existing).
- **New**: `fd-lock` (or `fs4`) for advisory file locks.
- **Testing**: `cargo test` (unit + `tests/`); `tempfile` for isolated roots; multi-process tests spawn the built binary (`env!("CARGO_BIN_EXE_project-map-cli-rust")`).

---

## Components and Interfaces

| Component | Change |
|:--|:--|
| `core::graph::Graph::load(path) -> Result<Graph>` | Pure: parse or `Err(AppError::CorruptIndex{path, source})`. No rebuild, no writes. Normalise node paths on load. |
| `core::graph::Graph::save(path)` | Atomic temp + fsync + rename. |
| `core::query_engine::QueryEngine::load_with_retry(path, policy)` | New: retries `CorruptIndex` / `NotFound` with backoff (policy injectable for tests). |
| `core::orchestrator::Orchestrator::build_index(root, out_dir)` | Signature gains `out_dir`; enforces the root/out invariant; excludes `out_dir` by canonical path; fills `meta`. |
| `core::orchestrator::Orchestrator::save_index_versioned(out_dir)` | Runs under `IndexLock`; ms+pid backup names; atomic `latest` swap; rotation keeps 5 and never removes the `latest` target. |
| `core::lock::IndexLock` (new) | `acquire(out_dir, timeout) -> Result<IndexLockGuard>`; RAII release; lock file `<out>/.lock`. |
| `core::utils::normalize_rel_path(root, p) -> String` (new) | Single normalisation helper (DRY): index, query and load all use it. |
| `core::watcher::ProjectWatcher` | Uses `IndexLock` and coalescing (skip rebuild if `latest` is newer than the event); on load failure keeps the old engine; `is_ignored_path` also checks `out_dir`. |
| `mcp::server` (`init`, `status`, path args) | `init` uses explicit root/out via the shared build routine (no duplicated build code with `main.rs` / watcher). `status` shows `meta` and warnings. Path args are normalised. |
| `main.rs` (`build`/`refresh`/`watch`/`status`/`query`…) | Same shared build routine; same status formatter (one source); CLI load uses retry and reports actionable errors. |
| `scaffold_if_empty(root, out_dir)` | Never writes inside `out_dir`. |

**DRY**: the "build + save + reload" sequence currently exists in three places (`main.rs` build/watch, `mcp::server` init, `watcher.rs`) plus the bogus fourth in `Graph::load`. It becomes one function, `core::orchestrator::rebuild_and_publish(root, out_dir) -> Result<QueryEngine>`.

---

## Data Models

### Index JSON (`.project-map.json`)
```json
{
  "meta": {
    "schema_version": 2,
    "tool_version": "0.2.0",
    "root": "/opt/wde",
    "built_at": "2026-10-12T15:18:07.123+01:00",
    "builder_pid": 1773051,
    "file_count": 4210,
    "symbol_count": 13530,
    "edge_count": 17019
  },
  "nodes": [ { "path": "utils/physics-lab/optimizer/optimizer.py", "name": "get_timeframe_search_space", "kind": "function", "line": 41, "...": "..." } ],
  "edges": [ ]
}
```
* `meta` uses `#[serde(default)]`, so older indexes load with `meta = None`. `status` then reports "legacy index (no meta)".
* **Path format change**: `path` is stored without a leading `./`. Tool output therefore shows `utils/x.py` rather than `./utils/x.py`. Inputs accept both forms.

### Lock file
`<out_dir>/.lock`: empty file used only for advisory locking. Created on demand and never deleted (deleting a lock file while it is in use is unsafe).

### Backups
`<out_dir>/backups/YYYYMMDD_HHMMSS_mmm_<pid>/.project-map.json`; `<out_dir>/latest` → relative symlink to one of them.

---

## Verification & Acceptance Criteria
1. **D1/D2 regression**: given a root with real source files and a truncated `latest/.project-map.json`, `QueryEngine::load_with_retry` returns `CorruptIndex`. Afterwards `out_dir` contents are byte-identical (no new backups, no `ARCHITECTURE.md`, `latest` unchanged).
2. **Recovery-root regression**: no code path calls `build_index` with a root derived from the index path. Enforced by the invariant error plus a test that `build_index(out_dir, out_dir)` fails.
3. **Atomicity**: a reader thread loading `latest` in a tight loop while a writer saves 50 times observes zero parse errors and zero missing-`latest` errors.
4. **Concurrency stress (multi-process)**: two `mcp -w` processes, or two `watch` processes, on one temp root with 200 source files; 200 rapid writes across files. Afterwards: (a) no reader ever saw a parse error or an index with < 95% of the files; (b) the final `latest` contains all 200 files; (c) at most 5 backups; (d) no `ARCHITECTURE.md` in `out_dir`.
5. **Coalescing**: under a burst of events, the number of full rebuilds across two watchers is at most the number of debounced bursts + 1 (no N× duplication).
6. **Path normalisation**: `utils/x.py`, `./utils/x.py` and `<root>/utils/x.py` give identical results for `query path=`, `fetch_symbol`, `blast_radius` and `plan`. A 0.1.18 fixture index with `./` paths loads and answers both forms.
7. **Status**: shows the `meta` fields; WARNING for an empty index and for an out-dir-only index (fixture copied from the observed 1.6 KB wipe); "legacy index" for a meta-less index.
8. **D7**: the outline for a file returns that file's own node name; the test fixture reproduces the mismatch before the fix.
9. **Backward/forward compatibility**: a 0.2.0 index parses with the 0.1.18 `SerializableGraph` (checked by a test using a copy of the old struct).
10. **Suite**: `cargo test` (all existing plus new tests), `cargo clippy -- -D warnings`, `cargo build --release` and `cargo publish --dry-run` all pass.
11. **Field check in `/opt/wde`** (after rollout): run with two MCP clients (Claude Code + AGY), repeat the reproduction probe (byte-identical rewrites of a tracked JSON file ×10). The index stays ≥ 6 MB and `status` counts are stable.

---

## Rollout (after publish — performed by the user / an agent in `/opt/wde`)
1. `cargo install project-map-cli-rust --version 0.2.0 --force` (or `cargo install --path ~/Projects/project-map-cli-rust --force`).
2. **Restart every MCP client** (Claude Code and AGY sessions), because running servers keep the old binary in memory. Check with `ps -eo pid,lstart,args | grep project-map` that every process started after the install.
3. In `/opt/wde`, call map `init` once, then `status`: counts are shown and there is no WARNING.
4. Run the field check (criterion 11).
5. Update `/opt/wde/docs/coding_assistant_setup.md` with the minimum map version and the restart note.
