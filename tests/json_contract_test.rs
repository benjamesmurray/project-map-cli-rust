//! Task 4.4: golden JSON for every tool. The files under `tests/fixtures/json_contract/`
//! pin the public contract; any added, removed or renamed field, or changed value, fails
//! the test with a diff. See the README there for the (breaking-change) update step.

mod common;

use common::*;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::tools::{run, Format, ToolCall, ToolEnv};
use project_map_cli_rust::mcp::server::McpServerHandler;
use rust_mcp_sdk::mcp_server::ServerHandler;
use rust_mcp_sdk::schema::CallToolRequestParams;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

const DOCS: &str = "tests/fixtures/docs";
const GOLDEN_DIR: &str = "tests/fixtures/json_contract";

/// Fields that name the machine, the clock or the build, replaced before comparison.
fn normalise(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                let placeholder = match key.as_str() {
                    "root" => Some("<root>"),
                    "built_at" => Some("<built_at>"),
                    "tool_version" => Some("<tool_version>"),
                    "commit" | "head_now" => Some("<sha>"),
                    _ => None,
                };
                match placeholder {
                    Some(p) if v.is_string() => *v = json!(p),
                    _ => normalise(v),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalise),
        _ => {}
    }
}

/// A line diff of two pretty-printed documents, for the failure message.
fn diff(want: &str, got: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    // Longest common subsequence table.
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j == b.len() || (i < a.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push_str(&format!("- {}\n", a[i]));
            i += 1;
        } else {
            out.push_str(&format!("+ {}\n", b[j]));
            j += 1;
        }
    }
    out
}

struct Case {
    /// The golden file's name, `<tool>[_<case>]`.
    name: &'static str,
    tool: &'static str,
    value: Value,
}

fn json_of(engine: Option<&QueryEngine>, root: &Path, call: ToolCall) -> Value {
    let env = ToolEnv {
        root,
        out: &root.join(".project-map"),
    };
    let out = run(engine, env, &call, Format::Json);
    let value = out.json.expect("JSON output");
    assert_eq!(value.to_string(), out.text);
    value
}

fn query_symbol(q: &str) -> ToolCall<'_> {
    ToolCall::Query {
        query: Some(q),
        path: None,
        filename: None,
        preview: Some(2),
        limit: None,
    }
}

fn query_path(p: &str) -> ToolCall<'_> {
    ToolCall::Query {
        query: None,
        path: Some(p),
        filename: None,
        preview: None,
        limit: None,
    }
}

fn docs_cases(engine: &QueryEngine, root: &Path) -> Vec<Case> {
    let j = |call| json_of(Some(engine), root, call);
    vec![
        Case {
            name: "status",
            tool: "status",
            value: j(ToolCall::Status { max_depth: None }),
        },
        Case {
            name: "status_depth1",
            tool: "status",
            value: j(ToolCall::Status { max_depth: Some(1) }),
        },
        Case {
            name: "query_symbol",
            tool: "query",
            value: j(query_symbol("append")),
        },
        Case {
            name: "query_path",
            tool: "query",
            value: j(query_path("src/audit/log.rs")),
        },
        Case {
            name: "query_filename",
            tool: "query",
            value: j(ToolCall::Query {
                query: None,
                path: None,
                filename: Some("audit/log"),
                preview: None,
                limit: None,
            }),
        },
        Case {
            name: "search",
            tool: "search",
            value: j(ToolCall::Search {
                query: "retention",
                preview: Some(1),
                limit: None,
            }),
        },
        Case {
            name: "search_limited",
            tool: "search",
            value: j(ToolCall::Search {
                query: "a",
                preview: None,
                limit: Some(2),
            }),
        },
        Case {
            name: "fetch_symbol",
            tool: "fetch_symbol",
            value: j(ToolCall::FetchSymbol {
                path: "src/audit/log.rs",
                symbol: "verify_chain",
            }),
        },
        Case {
            name: "blast_radius",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "src/audit/log.rs",
                symbol: "AuditLog",
                limit: None,
            }),
        },
        Case {
            name: "blast_radius_file",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "src/audit/log.rs",
                symbol: "no_such",
                limit: None,
            }),
        },
        Case {
            name: "blast_radius_not_found",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "src/none.rs",
                symbol: "x",
                limit: None,
            }),
        },
        Case {
            name: "plan",
            tool: "plan",
            value: j(ToolCall::Plan {
                symbol: "flush_buffer",
                limit: None,
            }),
        },
        Case {
            name: "doc_search",
            tool: "doc_search",
            value: j(ToolCall::DocSearch {
                query: "audit log",
                limit: None,
            }),
        },
        Case {
            name: "doc_outline",
            tool: "doc_outline",
            value: j(ToolCall::DocOutline {
                path: "docs/guide.md",
                max_depth: None,
            }),
        },
        Case {
            name: "doc_outline_depth1",
            tool: "doc_outline",
            value: j(ToolCall::DocOutline {
                path: "docs/guide.md",
                max_depth: Some(1),
            }),
        },
        Case {
            name: "doc_section",
            tool: "doc_section",
            value: j(ToolCall::DocSection {
                path: "docs/guide.md",
                anchor: "usage",
                max_chars: None,
            }),
        },
        Case {
            name: "doc_section_truncated",
            tool: "doc_section",
            value: j(ToolCall::DocSection {
                path: "docs/guide.md",
                anchor: "usage",
                max_chars: Some(20),
            }),
        },
        Case {
            name: "doc_links_code",
            tool: "doc_links",
            value: j(ToolCall::DocLinks {
                path: "src/audit/log.rs",
                symbol: None,
            }),
        },
        Case {
            name: "doc_links_doc",
            tool: "doc_links",
            value: j(ToolCall::DocLinks {
                path: "docs/links.md",
                symbol: None,
            }),
        },
        Case {
            name: "doc_terms",
            tool: "doc_terms",
            value: j(ToolCall::DocTerms {
                path_prefix: None,
                limit: None,
            }),
        },
        Case {
            name: "api_surface",
            tool: "api_surface",
            value: j(ToolCall::ApiSurface { path: None }),
        },
        Case {
            name: "modules",
            tool: "modules",
            value: j(ToolCall::Modules {
                path_prefix: None,
                max_depth: Some(2),
            }),
        },
        Case {
            name: "error_not_found",
            tool: "doc_outline",
            value: j(ToolCall::DocOutline {
                path: "docs/nope.md",
                max_depth: None,
            }),
        },
        Case {
            name: "error_invalid_argument",
            tool: "imports",
            value: j(ToolCall::Imports {
                package: "",
                path_prefix: None,
                limit: None,
            }),
        },
        Case {
            name: "error_index_missing",
            tool: "search",
            value: json_of(
                None,
                root,
                ToolCall::Search {
                    query: "x",
                    preview: None,
                    limit: None,
                },
            ),
        },
        Case {
            name: "status_index_missing",
            tool: "status",
            value: json_of(None, root, ToolCall::Status { max_depth: None }),
        },
    ]
}

fn imports_cases(engine: &QueryEngine, root: &Path) -> Vec<Case> {
    let j = |call| json_of(Some(engine), root, call);
    vec![
        Case {
            name: "imports_scope_prefix",
            tool: "imports",
            value: j(ToolCall::Imports {
                package: "@sentry/*",
                path_prefix: None,
                limit: None,
            }),
        },
        Case {
            name: "imports_package",
            tool: "imports",
            value: j(ToolCall::Imports {
                package: "stripe",
                path_prefix: None,
                limit: None,
            }),
        },
        Case {
            name: "imports_rust",
            tool: "imports",
            value: j(ToolCall::Imports {
                package: "serde_json",
                path_prefix: None,
                limit: None,
            }),
        },
        Case {
            name: "imports_limited",
            tool: "imports",
            value: j(ToolCall::Imports {
                package: "stripe",
                path_prefix: None,
                limit: Some(1),
            }),
        },
        Case {
            name: "blast_radius_diamond",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "diamond/target.ts",
                symbol: "target",
                limit: None,
            }),
        },
        Case {
            name: "blast_radius_rust",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "rust_crate/src/a/b.rs",
                symbol: "Sym",
                limit: None,
            }),
        },
        Case {
            name: "blast_radius_rust_file",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "rust_crate/src/a/b.rs",
                symbol: "no_such",
                limit: None,
            }),
        },
        Case {
            name: "blast_radius_rust_method",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "rust_crate/src/a/b.rs",
                symbol: "gamma",
                limit: None,
            }),
        },
        Case {
            name: "blast_radius_rust_free_function",
            tool: "blast_radius",
            value: j(ToolCall::BlastRadius {
                path: "rust_crate/src/user.rs",
                symbol: "sym_count",
                limit: None,
            }),
        },
        Case {
            name: "query_path_rust",
            tool: "query",
            value: j(query_path("rust_crate/src/user.rs")),
        },
        Case {
            name: "plan_rust",
            tool: "plan",
            value: j(ToolCall::Plan {
                symbol: "Gamma",
                limit: None,
            }),
        },
        Case {
            name: "modules_imports",
            tool: "modules",
            value: j(ToolCall::Modules {
                path_prefix: Some("rust_crate"),
                max_depth: Some(1),
            }),
        },
    ]
}

/// `init` over a Git repository, with the commit normalised.
async fn init_case(base: &Path) -> Case {
    let root = base.join("init-project");
    git_fixture(DOCS, &root);
    let handler = McpServerHandler::with_paths(
        root.canonicalize().unwrap(),
        base.join("init-out"),
        Arc::new(RwLock::new(None)),
    );
    let res = handler
        .handle_call_tool_request(
            CallToolRequestParams {
                name: "init".into(),
                arguments: json!({"format": "json"}).as_object().cloned(),
                meta: None,
                task: None,
            },
            Arc::new(DummyRuntime),
        )
        .await
        .unwrap();
    Case {
        name: "init",
        tool: "init",
        value: Value::Object(res.structured_content.unwrap()),
    }
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(GOLDEN_DIR).join(format!("{name}.json"))
}

#[tokio::test]
async fn json_output_matches_the_golden_contract_files() {
    // A fixed directory name keeps paths and the `root` field stable.
    let base = tempfile::tempdir().unwrap();
    let (docs_root, imports_root) = (
        copy_fixture(DOCS, &base.path().join("contract-docs")),
        copy_fixture(IMPORTS_FIXTURE, &base.path().join("contract-imports")),
    );
    let docs = build_engine(&docs_root, &base.path().join("b1"));
    let imports = build_engine(&imports_root, &base.path().join("b2"));

    let mut cases = docs_cases(&docs, &docs_root);
    cases.extend(imports_cases(&imports, &imports_root));
    cases.push(init_case(base.path()).await);

    // Every tool has a golden.
    let tools: BTreeSet<_> = cases.iter().map(|c| c.tool).collect();
    for tool in [
        "status",
        "query",
        "search",
        "fetch_symbol",
        "blast_radius",
        "plan",
        "doc_search",
        "doc_outline",
        "doc_section",
        "doc_links",
        "doc_terms",
        "api_surface",
        "modules",
        "imports",
        "init",
    ] {
        assert!(tools.contains(tool), "no golden case for {tool}");
    }

    let update = std::env::var("UPDATE_GOLDENS").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    let mut names = BTreeSet::new();
    for mut case in cases {
        assert!(names.insert(case.name), "duplicate case {}", case.name);
        assert_eq!(case.value["tool"], case.tool, "{}", case.name);
        normalise(&mut case.value);
        let got = serde_json::to_string_pretty(&case.value).unwrap() + "\n";
        let path = golden_path(case.name);
        if update {
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(want) if want == got => {}
            Ok(want) => failures.push(format!(
                "{}: differs from {}\n{}",
                case.name,
                path.display(),
                diff(&want, &got)
            )),
            Err(_) => failures.push(format!(
                "{}: no golden file {} (run with UPDATE_GOLDENS=1)",
                case.name,
                path.display()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "JSON contract changed. A change to a field name, field or value is breaking: bump \
         `contract`, add a CHANGELOG entry and rerun with UPDATE_GOLDENS=1 (see {GOLDEN_DIR}/README.md).\n\n{}",
        failures.join("\n")
    );

    // No stale golden files.
    if !update {
        let on_disk: BTreeSet<String> = std::fs::read_dir(GOLDEN_DIR)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter_map(|n| n.strip_suffix(".json").map(String::from))
            .collect();
        let wanted: BTreeSet<String> = names.iter().map(|n| n.to_string()).collect();
        assert_eq!(
            on_disk, wanted,
            "golden files without a case, or cases without a file"
        );
    }
}

#[test]
fn the_diff_shows_added_removed_and_changed_lines() {
    let d = diff("a\nb\nc\n", "a\nx\nc\nd\n");
    assert_eq!(d, "- b\n+ x\n+ d\n");
}

#[test]
fn normalise_replaces_only_the_machine_specific_fields() {
    let mut v = json!({
        "summary": {"root": "/tmp/x", "built_at": "2026", "commit": "abc", "head_now": null,
                    "tool_version": "0.4.0", "files": 3, "dirty": false},
        "index": {"commit": null, "schema_version": 4}
    });
    normalise(&mut v);
    assert_eq!(
        v,
        json!({
            "summary": {"root": "<root>", "built_at": "<built_at>", "commit": "<sha>",
                        "head_now": null, "tool_version": "<tool_version>", "files": 3,
                        "dirty": false},
            "index": {"commit": null, "schema_version": 4}
        })
    );
}
