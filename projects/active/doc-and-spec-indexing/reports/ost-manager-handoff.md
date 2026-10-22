# Hand-off to OST-Manager: project-map 0.3.0 (task 7.4)

Prepared 2026-10-22. Nothing in the OST-Manager repository was edited; the ROADMAP line below is a proposal for its owner.

## What 0.3.0 delivers

The scope of `Specification.md`: the six doc tools and their CLI subcommands, Markdown sections with GitHub-style anchors, frontmatter, doc → code `References`, OpenAPI operations and schemas, terms, schema v3 with stale handling, and the 13 use-case tool descriptions. Evidence is in `verification.md`.

## Status of `p4-project-map-uplift.md` (OST-Manager's additions)

The brief asked for ten additions "to take into P4's specification". The Specification was not extended with them, so most are **not in 0.3.0**. P5's adapter depends on several.

| # | Addition | Status in 0.3.0 |
|---|---|---|
| 1 | `format=json` with stable field names, structured MCP content, contract test | **Not built.** Output is TOON only. P5 "reads JSON, never parses TOON", so this blocks P5's repo adapter. |
| 2 | `status` reports the indexed Git commit and dirty flag | **Not built.** `status` shows schema and tool version, root, build time, counts and stale state, but no commit. Blocks "refuse to cite from an index not at the pinned SHA". |
| 3 | GitHub-exact anchors, duplicate suffixes | **Done.** `test_github_slug`, `test_duplicate_anchors`, `test_guide_anchors`. |
| 4 | `doc_outline(path, max_depth)`, `status(max_depth)` | **Not built.** Output caps (`max_chars`) exist; depth limits do not. `doc_terms` has `limit`. |
| 5 | Module sizes (file, symbol, doc-section counts per directory) | **Not built.** |
| 6 | `blast_radius` counts as numbers | **Not built.** (Also needs #1.) |
| 7 | Query for files importing an external package by name | **Not built; not investigated.** |
| 8 | Non-interactive stdio server; `status` answers immediately when the index is missing or stale; clean stderr | **Partly existing, not verified.** `mcp` runs over stdio and only watches with `--watch`. Stale/missing `status` and ANSI-free stderr were not checked for this release. |
| 9 | Stable order for equal scores (path, then line) | **Not verified.** `doc_terms` has a determinism test; ties in `doc_search` were not tested against this requirement. |
| 10 | Published to crates.io | **Pending the owner.** `cargo publish --dry-run` passes on a clean tree; publishing is the owner's step. |

Also unchanged from the brief: whether Claude Code defers the map MCP tools behind ToolSearch (the likely cause of the grep-to-map ratio) was not examined.

## What is true for P5 today

P5's ROADMAP gate reads "P4 done (released in project-map ≥ 0.3.0)". Publishing 0.3.0 will satisfy the letter of that gate. It will **not** give P5 JSON output or the indexed commit. Either those become a 0.3.x/0.4.0 follow-up before P5's repo adapter, or P5 consumes TOON for now.

## P5's first task (post-release acceptance, not a gate)

Hand-check `doc_terms` and `doc_outline` on OST-Manager's `docs/` as persona-extraction input; findings return as a 0.3.x follow-up in this repository.

Measurements to start from (index built from a scratch dir, `verification.md` 7.1): `doc terms docs` is 3,874 characters (about 970 tokens, 40 terms of which 30 shown); an outline is at most 978 characters (about 245 tokens); 193 doc files, 1,271 sections, 3,500 references. Mentions are noisy there (9,271 unresolved, 1,372 ambiguous), so expect `doc_links` to be sparse for prose-heavy ADRs.

## Proposed line for OST-Manager `projects/ROADMAP.md` (P4 row)

> **Released** project-map 0.3.0 (date of publishing): doc tools, GitHub anchors, tool descriptions. **Not in 0.3.0:** JSON output, indexed commit in `status`, depth limits, module sizes, blast-radius counts, package-import query (items 1, 2, 4-7 of `p4-project-map-uplift.md`); P5 phase 1 decides whether to wait for a 0.3.x with JSON or consume TOON first.

## Upgrade reminder for any machine running OST-Manager tooling

Restart every running `project-map-cli-rust mcp` process after installing 0.3.0 and before the first 0.3 build (README, CHANGELOG).
