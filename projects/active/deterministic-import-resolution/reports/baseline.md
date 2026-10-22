# 0.4.0 variance baseline (task 1.2)

Script: `measure.py` in this directory. Raw data: `baseline_0.4.0.json`. Command:

```bash
SELF_SNAPSHOT=<git archive a516255 extracted> python3 measure.py ~/.cargo/bin/project-map-cli-rust 0.4.0 baseline_0.4.0.json 5
```

Binary: installed 0.4.0 release build. Each repository is built 5 times into a scratch `--out`. Inputs: `self` is a `git archive` of `a516255` (the 0.4.0 tree); OST-Manager at `a7946e9` (working tree has uncommitted edits to `docs/decisions/*.md` only); `/opt/wde` as it stood on 2026-10-22. The five blast targets per repository are the five most-imported code files in a first 0.4.0 build, fixed in `measure.py`; for OST-Manager the first is `config.py::Settings`. "Total" is `summary.total` of `blast --format json`. Time is the median wall time of the 5 builds (the spec says median of 3).

## Results

| Repository | Edges | `Imports` edges | Resolved / unresolved imports | Build median (s) | Blast totals identical over 5 builds? |
|:--|--:|--:|:--|--:|:--|
| self | 2,452 | 292 | 194 / 233 | 0.257 | yes |
| OST-Manager | 8,129 | 657 | 657 / 482 | 0.241 | **no** |
| /opt/wde | 42,678 | 1,942 | 1,942 / 2,961 | 2.103 | **no** |

Edge, `Imports` edge and resolved counts are the same in all 5 builds on every repository, as the spec's background says (the edges point at different nodes, not at different numbers).

## Spread of `blast_radius` total over 5 builds (distinct values seen)

| Repository | Symbol | Values |
|:--|:--|:--|
| self | all five | stable (26, 10, 18, 19, 21) |
| OST-Manager | `config.py::Settings` | **115, 143** |
| OST-Manager | `ost/store.py` | **83, 132** |
| OST-Manager | `ost/errors.py` | **98, 134** |
| OST-Manager | `contracts/workspace.py` | 64 |
| OST-Manager | `llm/client.py` | 49 |
| /opt/wde | `viz-portal/backend/app/logging.py` | **171, 174, 175, 176** |
| /opt/wde | `utils/physics-lab/optimizer/optimizer.py` | **65, 73, 78, 83** |
| /opt/wde | `viz-portal/backend/app/api.py` | **24, 29** |
| /opt/wde | `Bar.kt` | 62 |
| /opt/wde | `PhaseLabelEvent.kt` | 71 |

The defect reproduces on OST-Manager (Settings 115 vs 143; the spec saw 115 vs 141) and is worse on `/opt/wde` (four distinct values for one file).

A first 0.4.0 build of `/opt/wde` also showed `Imports` edges into documentation files (a single ADR file with 181 inbound `Imports` edges): the loose suffix rule links imports to unrelated documents. Phase 3 checks these links by sampling.
