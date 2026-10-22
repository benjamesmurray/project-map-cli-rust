# Baseline: project-map-cli-rust 0.2.0

Measured 2026-10-21 for task 1.1, before any change in this feature. Task 7.1 repeats these measurements with 0.3.0.

## Setup

- Binary: `cargo build --release` at `e95381a` (0.2.0, no source changes), copied to the session scratchpad as `pm-0.2.0`.
- Machine: x86_64, 24 cores, Linux 6.11.
- Each run builds a full index from scratch into a scratch out dir, so it never touches the repositories' live `.project-map/` (running 0.2.x MCP servers are not involved):

```bash
rm -rf "$OUT" && /usr/bin/time -f "%e s %M KB" pm-0.2.0 build -r <repo> -o "$OUT"
pm-0.2.0 status -i "$OUT"
```

## Results

| Repository | Run 1 | Run 2 | Run 3 | **Median** | Peak RSS | Index size | Files | Symbols | Edges |
|---|---|---|---|---|---|---|---|---|---|
| `project-map-cli-rust` | 0.18 s | 0.16 s | 0.16 s | **0.16 s** | ~26 MB | 119,100 B | 72 | 332 | 343 |
| `/opt/wde` | 4.47 s | 4.50 s | 4.51 s | **4.50 s** | ~49 MB | 7,542,483 B | 2,897 | 17,634 | 19,567 |

Files of the types the new passes read:

| Repository | `.md` | `.yaml` | `.yml` | `.json` |
|---|---|---|---|---|
| `project-map-cli-rust` | 11 | 1 | 1 | 11 |
| `/opt/wde` | 1,628 | 7 | 6 | 440 |

## Notes for 7.1

- **20 % budget**: `/opt/wde` allows up to **5.40 s**. This repository's build is too short (0.16 s, ±0.02 s between runs) for a 20 % limit to be meaningful, so the `/opt/wde` figure is the one that decides it; for this repository report the change and treat anything within run-to-run noise as passing.
- `/opt/wde` has 1,628 Markdown files, so the doc pass's cost will show there. Watch the per-file cost of `pulldown-cmark` and of term extraction.
- The `status` root-mismatch WARNING in these runs is expected: `status` was run from the scratch directory, not the repository root.
