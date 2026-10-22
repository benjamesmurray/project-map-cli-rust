# 0.3.0 Baseline (task 1.1)

Date: 2026-10-22. Binary: `target/release/project-map-cli-rust` 0.3.0 (release build of `4d4a6e9`). Each build is a cold full build into a scratch `--out` (removed first), median of 3.

## Prerequisite check
Running servers: PIDs 1861039 (started 10:15) and 1962498 (11:19), both `~/.cargo/bin/project-map-cli-rust mcp -w`, installed 10:05, which is 0.3.0. No 0.2.x server runs. Met.

## Indexing
| Repository | Files | Symbols | Edges | Doc files | Doc sections | Build times (ms) | Median (ms) | Index JSON (bytes) |
|:--|--:|--:|--:|--:|--:|:--|--:|--:|
| this repo (`4d4a6e9`) | 98 | 694 | 1231 | 21 | 162 | 222, 226, 224 | 224 | 341,914 |
| /opt/wde (`652e2c9c`) | 2897 | 20378 | 42327 | 1628 | 9976 | 4651, 4660, 4658 | 4658 | 14,480,105 |

Commands:
```
for i in 1 2 3: rm -rf OUT; time project-map-cli-rust build --root ROOT --out OUT
du / ls -l OUT/latest/.project-map.json
```
The scratch index of /opt/wde was never built in its own `.project-map/`.

## TOON output sizes (this repo, scratch index)
Saved in `reports/baseline/<call>.toon`; 1.4 compares byte for byte (for `status`, ignoring the Index path, Built At and builder lines).

| Call | CLI command | Characters |
|:--|:--|--:|
| status | `status` | 2337 |
| search 'graph' | `search graph` | 704 |
| query path=src/core/graph.rs | `context -p src/core/graph.rs` | 1858 |
| blast_radius src/core/query_engine.rs load | `blast -p src/core/query_engine.rs -s load` | 374 |
| doc_outline README.md | `doc outline README.md` | 869 |
| doc_terms | `doc terms` | 3852 |

## Comparing later
The saved `.toon` files describe this repository as it was at `4d4a6e9`. Task 1.4 changes this tree, so a byte comparison against them is meaningless. 1.4 compared instead by building the 0.3.0 binary from `4d4a6e9` in a scratch worktree and running both binaries against the same current tree (13 CLI calls: the six above plus `find -q`, `find -f`, `impact`, `fetch`, `doc search`, `doc links` and `api`). Use the same method in 6.1.
