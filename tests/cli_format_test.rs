//! Task 4.3: `--format`, `--limit`, `--max-depth`, and the `modules` and `imports`
//! subcommands.

mod common;

use common::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
        .args(args)
        .output()
        .expect("failed to run the CLI")
}

fn ok_stdout(args: &[&str]) -> String {
    let out = cli(args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty(), "{args:?}: stderr {:?}", out.stderr);
    String::from_utf8(out.stdout).unwrap()
}

fn ok_json(args: &[&str]) -> Value {
    serde_json::from_str(&ok_stdout(args)).unwrap_or_else(|e| panic!("{args:?}: {e}"))
}

/// A fixture built with the binary itself. Returns (tempdir, index dir).
fn built(fixture: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = copy_fixture(fixture, &dir.path().join("project"));
    let out: PathBuf = dir.path().join("out");
    let build = cli(&[
        "build",
        "--root",
        root.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(build.status.success());
    let index = out.to_str().unwrap().to_string();
    (dir, index)
}

#[test]
fn every_query_subcommand_takes_format_json() {
    let (_d, idx) = built("tests/fixtures/docs");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["find", "--query", "audit"], "query"),
        (vec!["find", "--file", "audit/log"], "query"),
        (vec!["context", "--path", "src/audit/log.rs"], "query"),
        (vec!["impact", "--fqn", "AuditLog"], "plan"),
        (vec!["status"], "status"),
        (
            vec![
                "fetch",
                "--path",
                "src/audit/log.rs",
                "--symbol",
                "verify_chain",
            ],
            "fetch_symbol",
        ),
        (
            vec![
                "blast",
                "--path",
                "src/audit/log.rs",
                "--symbol",
                "AuditLog",
            ],
            "blast_radius",
        ),
        (vec!["search", "audit"], "search"),
        (vec!["doc", "search", "usage"], "doc_search"),
        (vec!["doc", "outline", "docs/guide.md"], "doc_outline"),
        (
            vec!["doc", "section", "docs/guide.md", "usage"],
            "doc_section",
        ),
        (vec!["doc", "links", "src/audit/log.rs"], "doc_links"),
        (vec!["doc", "terms"], "doc_terms"),
        (vec!["api"], "api_surface"),
        (vec!["modules"], "modules"),
        (vec!["imports", "serde"], "imports"),
    ];
    for (args, tool) in cases {
        let mut full = args.clone();
        full.extend(["--index", &idx, "--format", "json"]);
        let v = ok_json(&full);
        assert_eq!(v["tool"], tool, "{args:?}");
        assert_eq!(v["contract"], 1);
        assert_eq!(v["index"]["state"], "ok");
        assert!(v.get("error").is_none(), "{args:?}: {v}");

        // The default is TOON: not JSON.
        let mut toon = args.clone();
        toon.extend(["--index", &idx]);
        assert!(
            serde_json::from_str::<Value>(&ok_stdout(&toon)).is_err(),
            "{args:?}"
        );
    }
}

#[test]
fn json_matches_what_the_mcp_tool_returns() {
    use project_map_cli_rust::core::query_engine::QueryEngine;
    use project_map_cli_rust::core::tools::{run, Format, ToolCall, ToolEnv};
    let (_d, idx) = built("tests/fixtures/docs");
    let engine =
        QueryEngine::load(&Path::new(&idx).join("latest").join(".project-map.json")).unwrap();
    let env = ToolEnv {
        root: Path::new("."),
        out: Path::new(&idx),
    };
    let want = run(
        Some(&engine),
        env,
        &ToolCall::Search {
            query: "audit",
            preview: None,
            limit: Some(2),
        },
        Format::Json,
    );
    let got = ok_json(&[
        "search", "audit", "--limit", "2", "--index", &idx, "--format", "json",
    ]);
    assert_eq!(Some(got), want.json);
}

#[test]
fn limit_and_max_depth_flags() {
    let (_d, idx) = built("tests/fixtures/docs");

    let v = ok_json(&[
        "search", "a", "--limit", "1", "-i", &idx, "--format", "json",
    ]);
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    assert_eq!(v["truncated"], true);
    assert!(v["total"].as_u64().unwrap() > 1);

    let v = ok_json(&[
        "find", "--query", "a", "--limit", "1", "-i", &idx, "--format", "json",
    ]);
    assert_eq!(v["results"].as_array().unwrap().len(), 1);

    let v = ok_json(&[
        "impact", "--fqn", "AuditLog", "--limit", "1", "-i", &idx, "--format", "json",
    ]);
    assert!(v["results"].as_array().unwrap().len() <= 1);

    let v = ok_json(&[
        "blast",
        "--path",
        "src/audit/log.rs",
        "--symbol",
        "AuditLog",
        "--limit",
        "1",
        "-i",
        &idx,
        "--format",
        "json",
    ]);
    assert!(v["results"].as_array().unwrap().len() <= 1);

    let toon = ok_stdout(&["search", "a", "--limit", "1", "-i", &idx]);
    assert!(toon.contains("[limit 1:"), "{toon}");

    let v = ok_json(&["status", "--max-depth", "1", "-i", &idx, "--format", "json"]);
    assert_eq!(v["truncated"], true);
    assert!(
        ok_stdout(&["status", "--max-depth", "1", "-i", &idx]).contains("[tree cut at depth 1]")
    );

    let v = ok_json(&[
        "doc",
        "outline",
        "docs/guide.md",
        "--max-depth",
        "1",
        "-i",
        &idx,
        "--format",
        "json",
    ]);
    assert_eq!(v["truncated"], true);
    assert!(v["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["depth"] == 1));
    let toon = ok_stdout(&[
        "doc",
        "outline",
        "docs/guide.md",
        "--max-depth",
        "1",
        "-i",
        &idx,
    ]);
    assert!(toon.contains("[outline cut at depth 1]"), "{toon}");
}

#[test]
fn doc_section_json_is_whole_unless_max_chars_is_given() {
    let (_d, idx) = built("tests/fixtures/docs");
    let whole = ok_json(&[
        "doc",
        "section",
        "docs/guide.md",
        "usage",
        "-i",
        &idx,
        "--format",
        "json",
    ]);
    assert_eq!(whole["truncated"], false);
    let cut = ok_json(&[
        "doc",
        "section",
        "docs/guide.md",
        "usage",
        "--max-chars",
        "5",
        "-i",
        &idx,
        "--format",
        "json",
    ]);
    assert_eq!(cut["truncated"], true);
    assert_eq!(cut["summary"]["text"].as_str().unwrap().chars().count(), 5);
}

#[test]
fn modules_subcommand() {
    let (_d, idx) = built(IMPORTS_FIXTURE);
    let toon = ok_stdout(&["modules", "--max-depth", "1", "-i", &idx]);
    assert!(
        toon.starts_with("Resource: Modules | Prefix: . | Max Depth: 1"),
        "{toon}"
    );

    let v = ok_json(&[
        "modules",
        "rust_crate",
        "--max-depth",
        "1",
        "-i",
        &idx,
        "--format",
        "json",
    ]);
    assert_eq!(v["summary"]["path_prefix"], "rust_crate");
    let paths: Vec<_> = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths[0], "rust_crate");
    assert!(paths.contains(&"rust_crate/src"));
    assert!(
        paths.windows(2).all(|w| w[0] < w[1]),
        "sorted by path: {paths:?}"
    );
}

#[test]
fn imports_subcommand() {
    let (_d, idx) = built(IMPORTS_FIXTURE);
    let v = ok_json(&["imports", "@sentry/*", "-i", &idx, "--format", "json"]);
    assert!(v["summary"]["imports"].as_u64().unwrap() >= 2);
    assert!(v["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["package"].as_str().unwrap().starts_with("@sentry/")));

    let v = ok_json(&[
        "imports", "stripe", "--limit", "1", "-i", &idx, "--format", "json",
    ]);
    assert_eq!(v["results"].as_array().unwrap().len(), 1);

    let v = ok_json(&[
        "imports",
        "stripe",
        "--path-prefix",
        "ts",
        "-i",
        &idx,
        "--format",
        "json",
    ]);
    assert!(v["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["path"].as_str().unwrap().starts_with("ts/")));

    let toon = ok_stdout(&["imports", "serde_json", "-i", &idx]);
    assert!(
        toon.starts_with("Resource: Imports | Package: serde_json"),
        "{toon}"
    );
}

#[test]
fn json_errors_are_an_envelope_on_stdout_and_a_non_zero_exit() {
    let (_d, idx) = built("tests/fixtures/docs");
    let out = cli(&["doc", "outline", "nope.md", "-i", &idx, "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "not_found");
    assert!(out.stderr.is_empty());

    // No index at all.
    let out = cli(&[
        "search",
        "x",
        "-i",
        "/nonexistent/index",
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "index_missing");
    assert_eq!(v["index"]["state"], "missing");

    // `status` still answers with no index.
    let v = ok_json(&["status", "-i", "/nonexistent/index", "--format", "json"]);
    assert_eq!(v["index"]["state"], "missing");

    // TOON errors keep exit 0.
    let out = cli(&["doc", "outline", "nope.md", "-i", &idx]);
    assert!(out.status.success());
}

#[test]
fn format_is_rejected_where_it_does_not_apply_and_validated_where_it_does() {
    let (_d, idx) = built("tests/fixtures/docs");
    for args in [
        vec!["build", "--format", "json"],
        vec!["refresh", "--format", "json"],
        vec!["watch", "--format", "json"],
        vec!["mcp", "--format", "json"],
    ] {
        assert!(!cli(&args).status.success(), "{args:?}");
    }
    let out = cli(&["search", "x", "-i", &idx, "--format", "yaml"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
