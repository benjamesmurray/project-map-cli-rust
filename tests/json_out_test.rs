//! Task 4.1: the JSON envelope for every tool, on the docs and imports fixtures.

mod common;

use common::*;
use project_map_cli_rust::core::tools::{run, Format, ToolCall, ToolEnv};
use serde_json::Value;
use std::path::Path;

const DOCS_FIXTURE: &str = "tests/fixtures/docs";

fn json(engine: &project_map_cli_rust::core::query_engine::QueryEngine, call: ToolCall) -> Value {
    let env = ToolEnv {
        root: Path::new("."),
        out: Path::new(".project-map"),
    };
    let out = run(Some(engine), env, &call, Format::Json);
    let parsed: Value = serde_json::from_str(&out.text).expect("text is JSON");
    assert_eq!(Some(&parsed), out.json.as_ref(), "text equals structured");
    parsed
}

fn assert_envelope(v: &Value, tool: &str) {
    assert_eq!(v["tool"], tool);
    assert_eq!(v["contract"], 1);
    assert_eq!(v["index"]["state"], "ok");
    assert!(v["index"]["schema_version"].is_number());
    assert!(v["truncated"].is_boolean(), "{tool}: truncated");
    assert!(v["total"].is_number(), "{tool}: total");
    assert!(v["summary"].is_object(), "{tool}: summary");
    assert!(v["results"].is_array(), "{tool}: results");
    assert!(v.get("error").is_none(), "{tool}: unexpected error {v}");
}

#[test]
fn every_code_tool_returns_the_envelope() {
    let (_d, engine) = engine_for_fixture(DOCS_FIXTURE, "project");

    let v = json(
        &engine,
        ToolCall::Search {
            query: "audit",
            preview: None,
            limit: None,
        },
    );
    assert_envelope(&v, "search");
    let first = &v["results"][0];
    for key in ["symbol", "kind", "node_type", "path", "line", "end_line"] {
        assert!(first.get(key).is_some(), "search item lacks {key}");
    }

    let v = json(
        &engine,
        ToolCall::Query {
            query: None,
            path: Some("src/audit/log.rs"),
            filename: None,
            preview: None,
            limit: None,
        },
    );
    assert_envelope(&v, "query");
    assert_eq!(v["summary"]["path"], "src/audit/log.rs");

    let v = json(
        &engine,
        ToolCall::Query {
            query: None,
            path: None,
            filename: Some("audit/log"),
            preview: None,
            limit: None,
        },
    );
    assert_envelope(&v, "query");
    assert_eq!(v["results"][0]["path"], "src/audit/log.rs");

    let v = json(
        &engine,
        ToolCall::FetchSymbol {
            path: "src/audit/log.rs",
            symbol: "verify_chain",
        },
    );
    assert_envelope(&v, "fetch_symbol");
    assert!(v["summary"]["source"]
        .as_str()
        .unwrap()
        .contains("verify_chain"));

    let v = json(
        &engine,
        ToolCall::BlastRadius {
            path: "src/audit/log.rs",
            symbol: "AuditLog",
            limit: None,
        },
    );
    assert_envelope(&v, "blast_radius");
    for key in [
        "direct",
        "transitive",
        "total",
        "files_affected",
        "languages",
        "matched",
    ] {
        assert!(v["summary"].get(key).is_some(), "blast summary lacks {key}");
    }

    let v = json(
        &engine,
        ToolCall::Plan {
            symbol: "AuditLog",
            limit: None,
        },
    );
    assert_envelope(&v, "plan");
    assert_eq!(v["summary"]["requested"], "AuditLog");

    let v = json(
        &engine,
        ToolCall::Modules {
            path_prefix: None,
            max_depth: Some(1),
        },
    );
    assert_envelope(&v, "modules");
    assert_eq!(v["summary"]["max_depth"], 1);

    let v = json(
        &engine,
        ToolCall::Imports {
            package: "serde",
            path_prefix: None,
            limit: None,
        },
    );
    assert_envelope(&v, "imports");
}

#[test]
fn every_doc_tool_returns_the_envelope() {
    let (_d, engine) = engine_for_fixture(DOCS_FIXTURE, "project");

    let v = json(
        &engine,
        ToolCall::DocSearch {
            query: "usage",
            limit: None,
        },
    );
    assert_envelope(&v, "doc_search");
    assert!(v["results"][0]["anchor"].is_string());

    let v = json(
        &engine,
        ToolCall::DocOutline {
            path: "docs/guide.md",
            max_depth: Some(1),
        },
    );
    assert_envelope(&v, "doc_outline");
    assert_eq!(v["truncated"], true);
    assert!(v["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["depth"] == 1));

    let v = json(
        &engine,
        ToolCall::DocSection {
            path: "docs/guide.md",
            anchor: "usage",
            max_chars: None,
        },
    );
    assert_envelope(&v, "doc_section");
    assert!(v["summary"]["text"].is_string());
    assert_eq!(v["results"], serde_json::json!([]));

    let v = json(
        &engine,
        ToolCall::DocLinks {
            path: "src/audit/log.rs",
            symbol: None,
        },
    );
    assert_envelope(&v, "doc_links");

    let v = json(
        &engine,
        ToolCall::DocTerms {
            path_prefix: None,
            limit: None,
        },
    );
    assert_envelope(&v, "doc_terms");

    let v = json(&engine, ToolCall::ApiSurface { path: None });
    assert_envelope(&v, "api_surface");
}

#[test]
fn status_envelope_carries_the_summary_and_no_results() {
    let (_d, engine) = engine_for_fixture(DOCS_FIXTURE, "project");
    let v = json(&engine, ToolCall::Status { max_depth: Some(1) });
    assert_envelope(&v, "status");
    assert_eq!(v["results"], serde_json::json!([]));
    assert_eq!(v["truncated"], true);
    for key in [
        "tool_version",
        "schema_version",
        "root",
        "built_at",
        "commit",
        "dirty",
        "branch",
        "head_now",
        "dirty_now",
        "at_head",
        "stale",
        "files",
        "symbols",
        "edges",
        "doc_files",
        "doc_sections",
        "doc_references",
        "api_operations",
        "tree",
        "active_features",
    ] {
        assert!(
            v["summary"].get(key).is_some(),
            "status summary lacks {key}"
        );
    }
}

#[test]
fn imports_and_blast_on_the_imports_fixture() {
    let (_d, engine) = engine_for_fixture(IMPORTS_FIXTURE, "project");
    let v = json(
        &engine,
        ToolCall::Imports {
            package: "@sentry/*",
            path_prefix: None,
            limit: None,
        },
    );
    assert_envelope(&v, "imports");
    assert!(v["summary"]["imports"].as_u64().unwrap() >= 2);
    assert_eq!(v["total"], v["summary"]["imports"]);

    let v = json(
        &engine,
        ToolCall::BlastRadius {
            path: "diamond/target.ts",
            symbol: "target",
            limit: Some(1),
        },
    );
    assert_envelope(&v, "blast_radius");
    // The limit cuts the list, not the counts.
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    assert_eq!(v["truncated"], true);
    assert_eq!(v["summary"]["total"], 3);
    assert_eq!(v["total"], 3);
}

#[test]
fn errors_use_the_error_envelope() {
    let (_d, engine) = engine_for_fixture(DOCS_FIXTURE, "project");
    let env = ToolEnv {
        root: Path::new("."),
        out: Path::new(".project-map"),
    };

    let out = run(
        Some(&engine),
        env,
        &ToolCall::DocOutline {
            path: "docs/nope.md",
            max_depth: None,
        },
        Format::Json,
    );
    assert!(out.is_error);
    let v = out.json.unwrap();
    assert_eq!(v["error"]["code"], "not_found");
    assert!(v.get("results").is_none());

    let out = run(
        Some(&engine),
        env,
        &ToolCall::Imports {
            package: "",
            path_prefix: None,
            limit: None,
        },
        Format::Json,
    );
    assert_eq!(out.json.unwrap()["error"]["code"], "invalid_argument");

    let out = run(
        None,
        env,
        &ToolCall::Search {
            query: "x",
            preview: None,
            limit: None,
        },
        Format::Json,
    );
    assert!(out.is_error);
    let v = out.json.unwrap();
    assert_eq!(v["error"]["code"], "index_missing");
    assert_eq!(v["index"]["state"], "missing");
    assert!(v["index"]["commit"].is_null());

    let out = run(
        None,
        env,
        &ToolCall::Status { max_depth: None },
        Format::Json,
    );
    assert!(!out.is_error);
    let v = out.json.unwrap();
    assert_eq!(v["index"]["state"], "missing");
    assert!(v["summary"]["next_step"].is_string());
}

#[test]
fn the_json_default_limits_are_50_and_200() {
    use project_map_cli_rust::core::tools::{JSON_LIMIT_SEARCH, JSON_LIMIT_WALK};
    assert_eq!((JSON_LIMIT_SEARCH, JSON_LIMIT_WALK), (50, 200));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let mut src = String::new();
    for i in 0..80 {
        src.push_str(&format!("pub fn widget_{i}() {{}}\n"));
    }
    std::fs::write(root.join("w.rs"), src).unwrap();
    let engine = build_engine(&root, dir.path());
    let v = json(
        &engine,
        ToolCall::Search {
            query: "widget",
            preview: None,
            limit: None,
        },
    );
    assert_eq!(v["results"].as_array().unwrap().len(), 50);
    assert_eq!(v["total"], 80);
    assert_eq!(v["truncated"], true);
    let v = json(
        &engine,
        ToolCall::Search {
            query: "widget",
            preview: None,
            limit: Some(80),
        },
    );
    assert_eq!(v["results"].as_array().unwrap().len(), 80);
    assert_eq!(v["truncated"], false);
}
