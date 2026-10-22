# project-map 0.4.1: hand-off for OST-Manager

## What 0.4.1 fixes
OST-Manager's verification of 0.4.0 found that two builds of one tree could give different `blast_radius` results. Cause: the suffix-match fallback of import resolution took the first of several matching files in a `HashMap`, whose order is random per build. 0.4.1 ranks every candidate on whole dot-separated segments, picks the nearest, and **leaves a tie unlinked**. The file walk is also sorted, so the index is byte-stable apart from `built_at`, `builder_pid`, `root` and the Git fields. No new tools, no JSON contract change (`contract` 1), no schema change (4).

## OST-Manager before and after (HEAD `a7946e9`, 5 builds each)

| | 0.4.0 | 0.4.1 |
|:--|:--|:--|
| `Imports` edges | 657 | 628 |
| Resolved / unresolved imports | 657 / 482 | 628 / 511 |
| Ambiguous (tied, unlinked) | n/a | 0 |
| `config.py::Settings` blast total | 115 or 143 | **115** |
| `ost/store.py` | 83 or 132 | **77** |
| `ost/errors.py` | 98 or 134 | **92** |
| `contracts/workspace.py` | 64 | 58 |
| `llm/client.py` | 49 | 43 |
| Build time (median) | 0.235 s | 0.225 s |

The 29 imports that became unlinked are `re` (24), `redis` (4) and `string` (1): 0.4.0 linked standard-library and third-party names to unrelated project files. Totals that fall did so because those wrong dependents are gone. Two directory copies of `src/`, `docs/` and `tests/` give identical `blast_radius` JSON for `Settings`.

## What to change on your side
- Install 0.4.1, **restart every running `project-map-cli-rust mcp` process**, then run `project-map refresh` once per workspace.
- Recorded runs (P5) and risk factors (P8) that stored 0.4.0 blast numbers should be re-baselined; the old numbers were not reproducible.
- `imports`: a Python package is its first segment, so `imports google` finds `google.cloud`; `imports "google.*"` does not (the prefix form is for scoped JS packages).
- Ambiguous imports show as `resolved: false`. `PROJECT_MAP_LOG=debug project-map refresh` prints the first 20 ties with their candidates on stderr.

## Proposed OST-Manager ROADMAP lines (not applied; that repository was not edited)
- P4 row: `project-map 0.4.1 released (deterministic import resolution; supersedes 0.4.0 for blast_radius)`.
- P5 gate: `released in project-map >= 0.4.1`.

Full numbers, link samples and the specification mapping: `verification.md`.
