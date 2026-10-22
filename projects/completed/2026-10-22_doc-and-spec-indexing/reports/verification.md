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

## 7.2 Quality gate and spec coverage

Gate (2026-10-22): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and the full `cargo test` (lib, `docs_index_test`, `docs_query_test`, integration, mcp, regression, watcher) are clean. 161 tests pass (77 lib, 14 `docs_index_test`, 31 `docs_query_test`, 15 integration, 14 mcp, 11 regression, 9 watcher); 13 doc-tests are ignored.

**Gap found and fixed during the walk:** `get_file_outline` followed every outgoing edge of a file, so a Markdown file's doc → code `References` edge (a mention before the first heading) showed up in `query path=docs/links.md` as an unlabelled `File` entry. References are now skipped in outlines (`doc_links` is where they belong). The code-only constraint also had no test for `analyze_impact`, `find_symbol_in_path` and `get_symbol_count`; `every_existing_code_tool_skips_doc_and_api_nodes` now covers them.

### In-scope bullets

| Spec item | Task | Tests |
|---|---|---|
| Markdown sections, anchors, `Contains` edges, duplicate anchors | 2.1, 4.1 | `docs::tests::test_guide_section_tree`, `test_guide_anchors`, `test_duplicate_anchors`, `test_github_slug`, `test_section_ranges_nest`, `test_text_before_first_heading_and_fenced_headings`; `doc_pass_builds_section_tree_with_anchors` |
| Frontmatter (all keys, scalar/list `tags` and `supersedes`) | 2.2 | `test_frontmatter_every_key`, `test_frontmatter_typed_scalars_and_lists`, `test_frontmatter_absent_malformed_or_unrecognised`; `doc_pass_stores_frontmatter_by_normalised_path`; `search_docs_frontmatter_only_match_surfaces_the_first_section` |
| Link rules 1-5 and the non-links | 4.2 | `test_rule1_*` … `test_rule5_*`, `test_rule3_symbol_missing_or_repeated_in_the_file`, `test_rule4_owner_not_found_or_member_outside_it`, `test_rule5_short_and_denylisted_names_are_not_linked_even_if_unique` |
| Deepest-section attribution; pre-heading mention to the file | 2.3, 4.3 | `test_mentions_attributed_to_deepest_section_only`; `link_pass_adds_references_from_the_deepest_section_only` |
| Unresolved/ambiguous counting, fenced-bare-name exception | 4.3 | `test_unmatched_counting_skips_unresolved_fenced_bare_names_only`; `link_pass_counts_match_the_hand_counted_fixture` |
| OpenAPI detection, operations, schemas, no generic YAML symbols, 1 MB cap | 3.1-3.3 | `test_openapi_detection_by_name_and_content`, `test_openapi_detection_skips_large_files`, `test_openapi3_yaml_extraction`, `test_swagger2_json_extraction`, `parser::tests::test_openapi_yaml_skips_generic_symbols`, `test_non_openapi_yaml_unchanged`; `doc_pass_adds_api_nodes_under_the_spec_file` |
| Terms (stopwords, bold, order, caps 50 and 5, per directory) | 2.4, 4.4 | `test_term_runs_and_stopwords`, `test_term_sources`, `test_terms_*`; `doc_terms_are_stored_per_directory`, `doc_terms_are_byte_identical_across_builds` |
| Six MCP tools, defaults, truncation | 5.x, 6.1 | `docs_query_test` (31 tests), `test_mcp_doc_tools_return_the_formatter_output`, `test_mcp_doc_section_truncates_with_a_small_budget`, `test_mcp_doc_tools_report_errors_as_text` |
| `search` stays code-only, points to `doc_search` | 5.x, 6.2 | `code_search_ignores_documentation`, `test_mcp_every_tool_description_names_its_use_case` |
| `status` doc counts | 4.6 | `status_reports_doc_counts_for_a_current_index`, `status_on_a_schema_2_index_shows_the_stale_line_not_doc_counts` |
| Tool descriptions (13, use case named) | 6.2 | `test_mcp_every_tool_description_names_its_use_case`, `test_mcp_existing_tool_schemas_are_unchanged` |
| CLI `doc …` and `api` | 6.3 | `test_cli_doc_subcommands_match_the_mcp_tools`, `test_cli_help_lists_the_doc_subcommands` |

### Technical constraints

| Constraint | Evidence |
|---|---|
| `pulldown-cmark`, `serde_yaml`, no full OpenAPI model | `Cargo.toml`; `docs.rs` |
| `schema_version` 3; old/no-meta index read-only, stale message, never rewritten | `test_stale_index_detected_and_never_rewritten`, `every_doc_query_returns_the_stale_message_on_an_old_index_and_writes_nothing`, `test_fresh_index_is_not_stale` |
| `IndexMeta` counts `#[serde(default)]` | `graph::tests::test_schema_2_meta_without_doc_counts_defaults_to_zero` |
| Forward guard (`NewerIndex`), watcher skips | `test_forward_guard_refuses_to_overwrite_newer_index`, `test_watcher_skips_rebuild_over_newer_index` |
| Rollout note in README and CHANGELOG | **7.3** |
| Section text not stored | `index_json_holds_no_section_body_text` |
| Output cap 4,000 with marker | `toon::tests::test_truncate_*`, `doc_section_truncates_with_the_marker`, `doc_links_truncates`, `search_docs_formats_hits_and_truncates` |
| Indexing time growth at most 20 % | 7.1 above: wde -1 %, this repo +5 % |
| Existing code tools stay code-only | `code_search_ignores_documentation`, `every_existing_code_tool_skips_doc_and_api_nodes` |
| Unknown node/edge types do not break 0.2.x readers | `graph::tests::test_doc_node_types_round_trip` |

### Verification criteria

| # | Evidence |
|---|---|
| 1 | The `docs::tests` and `docs_index_test` rows above |
| 2 | OpenAPI rows above; `test_openapi_detection_skips_large_files` |
| 3 | Compatibility rows above |
| 4 | `mcp_test`: formatter output, truncation, stale message on a v2 index, errors |
| 5 | 7.1 table |
| 6 | `test_mcp_every_tool_description_names_its_use_case` |
| 7 | **7.3** |

### User stories

| Story | Test |
|---|---|
| `doc_search("audit log")` | `search_docs_audit_log_returns_the_expected_sections_in_order` |
| outline + terms under 2,000 tokens; OST-Manager measured | `outlines_and_terms_for_the_fixture_stay_under_2000_tokens`; 7.1 (≈ 970 tokens for terms) |
| `doc_links` both directions | `doc_links_code_to_docs_for_a_symbol`, `doc_links_docs_to_code_for_an_adr` |
| `api_surface` lines and schemas | `api_surface_lists_operations_and_schemas_for_yaml_and_json_specs` |

Remaining open items are the README and CHANGELOG (7.3) and the post-release OST-Manager hand check (7.4).
