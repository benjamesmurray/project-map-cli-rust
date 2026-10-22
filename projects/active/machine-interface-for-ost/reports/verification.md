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
