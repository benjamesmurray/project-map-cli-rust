# Verification: project-map-cli-rust 0.3.0

## 7.1 Performance and size (Verification 5)

Measured 2026-10-22 on the same machine as `baseline.md`, with `cargo build --release` at `9343118` (Cargo.toml still says 0.2.0 until 7.3). Each run is a full index from scratch into a scratch out dir, so no live `.project-map/` was touched and no running MCP server was involved.

```bash
rm -rf "$OUT" && /usr/bin/time -f "%e s %M KB" target/release/project-map-cli-rust build -r <repo> -o "$OUT"
```

| Repository | 0.2.0 median | 0.3.0 runs | 0.3.0 median | Growth | Limit | Result |
|---|---|---|---|---|---|---|
| `/opt/wde` | 4.50 s | 4.46, 4.43, 4.61 s | **4.46 s** | -1 % | +20 % (5.40 s) | **pass** |
| `project-map-cli-rust` (today's tree) | 0.19 s (0.2.0 binary, same tree) | 0.21, 0.20, 0.21 s | **0.21 s** | +5 % (about +0.01 s) | +20 % | **pass** |

The 0.2.0 baseline for this repository (0.16 s) was taken on the 72-file tree before the feature; the tree now has 92 files, so the like-for-like comparison is the 0.2.0 binary on today's tree (0.19, 0.20, 0.19 s, index 253,255 B).

| Repository | Peak RSS 0.2.0 → 0.3.0 | Index size 0.2.0 → 0.3.0 | Edges |
|---|---|---|---|
| `/opt/wde` | 49 MB → 79 MB | 7,542,483 B → 13,694,910 B (+82 %) | 19,567 → 41,147 |
| `project-map-cli-rust` (same tree) | 26 MB → 40 MB | 253,255 B → 308,733 B (+22 %) | n/a → 1,027 |

Symbols are unchanged on `/opt/wde` (17,634): doc and API nodes are counted separately.

New doc counts (`status`):

| Repository | Doc files | Doc sections | Doc references | Unresolved | Ambiguous | API operations |
|---|---|---|---|---|---|---|
| `/opt/wde` | 1,628 | 9,976 | 11,405 | 46,245 | 2,611 | 149 |
| `project-map-cli-rust` | 17 | 125 | 203 | 647 | 7 | 8 |
| `OST-Manager` | 193 | 1,271 | 3,500 | 9,271 | 1,372 | n/a |

Index size and memory grow more than time does, because the index now stores sections, terms and reference edges.

### OST-Manager `docs/` output size (user story 2)

Index built from a scratch out dir (the repository's working tree was only read).

| Output | Chars | ≈ Tokens (chars / 4) |
|---|---|---|
| `doc terms docs` (default 30 terms) | 3,874 | **≈ 970** |
| `doc outline` for all 105 `.md` files under `docs/`, summed | 65,605 | ≈ 16,400 |
| largest single outline | 978 | ≈ 245 |

Terms plus one outline fits well inside a few hundred to ~1,200 tokens. Quality of the terms (for persona extraction) is the hand check in 7.4, not judged here.
