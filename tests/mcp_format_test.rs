//! Task 4.2: the `format` argument, `structured_content`, the new tools, the typed
//! errors and the server instructions.

mod common;

use common::*;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::mcp::server::{server_details, McpServerHandler};
use rust_mcp_sdk::mcp_server::ServerHandler;
use rust_mcp_sdk::schema::{CallToolRequestParams, CallToolResult};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

fn handler_over(engine: Option<QueryEngine>) -> McpServerHandler {
    McpServerHandler::new(Arc::new(RwLock::new(engine)))
}

async fn call(handler: &McpServerHandler, name: &str, args: Value) -> CallToolResult {
    handler
        .handle_call_tool_request(
            CallToolRequestParams {
                name: name.to_string(),
                arguments: args.as_object().cloned(),
                meta: None,
                task: None,
            },
            Arc::new(DummyRuntime),
        )
        .await
        .unwrap()
}

fn text_of(res: &CallToolResult) -> String {
    serde_json::to_value(&res.content[0]).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Every tool but `init`, with arguments that succeed on the docs fixture.
fn tool_calls() -> Vec<(&'static str, Value)> {
    vec![
        ("status", json!({})),
        ("query", json!({"query": "audit"})),
        ("query", json!({"path": "src/audit/log.rs"})),
        ("query", json!({"filename": "audit/log"})),
        ("search", json!({"query": "audit"})),
        (
            "fetch_symbol",
            json!({"path": "src/audit/log.rs", "symbol": "verify_chain"}),
        ),
        (
            "blast_radius",
            json!({"path": "src/audit/log.rs", "symbol": "AuditLog"}),
        ),
        ("plan", json!({"symbol": "AuditLog"})),
        ("doc_search", json!({"query": "usage"})),
        ("doc_outline", json!({"path": "docs/guide.md"})),
        (
            "doc_section",
            json!({"path": "docs/guide.md", "anchor": "usage"}),
        ),
        ("doc_links", json!({"path": "src/audit/log.rs"})),
        ("doc_terms", json!({})),
        ("api_surface", json!({})),
        ("modules", json!({})),
        ("imports", json!({"package": "serde"})),
    ]
}

#[test]
fn initialize_carries_instructions() {
    let details = server_details();
    let text = details.instructions.expect("instructions are set");
    assert!(
        text.len() < 4096,
        "Claude Code cuts instructions at 4,096 characters"
    );
    assert!(text.contains("instead of grep"));
    for tool in [
        "status",
        "query",
        "search",
        "fetch_symbol",
        "blast_radius",
        "plan",
        "imports",
        "modules",
        "doc_search",
        "doc_outline",
        "doc_section",
        "doc_links",
        "doc_terms",
        "api_surface",
        "init",
    ] {
        assert!(text.contains(tool), "instructions do not name {tool}");
    }
    assert!(text.contains("json"));
}

#[tokio::test]
async fn every_tool_answers_in_both_formats() {
    let (_d, engine) = engine_for_fixture("tests/fixtures/docs", "project");
    let handler = handler_over(Some(engine));
    for (tool, args) in tool_calls() {
        let toon = call(&handler, tool, args.clone()).await;
        assert!(
            toon.structured_content.is_none(),
            "{tool}: toon has no structured content"
        );
        assert!(!text_of(&toon).is_empty(), "{tool}: toon text");

        let mut with_format = args.clone();
        with_format["format"] = json!("json");
        let res = call(&handler, tool, with_format).await;
        let parsed: Value = serde_json::from_str(&text_of(&res))
            .unwrap_or_else(|e| panic!("{tool}: text is not JSON: {e}"));
        let structured = res
            .structured_content
            .clone()
            .unwrap_or_else(|| panic!("{tool}: no structured content"));
        assert_eq!(
            parsed,
            Value::Object(structured),
            "{tool}: structured equals text"
        );
        assert_eq!(parsed["tool"], tool);
        assert_eq!(parsed["contract"], 1);
        assert_ne!(res.is_error, Some(true), "{tool}: {parsed}");
        assert!(parsed["results"].is_array(), "{tool}");

        // An explicit "toon" is the default.
        let mut explicit = args;
        explicit["format"] = json!("toon");
        assert_eq!(
            text_of(&call(&handler, tool, explicit).await),
            text_of(&toon),
            "{tool}"
        );
    }
}

#[tokio::test]
async fn an_invalid_format_is_invalid_argument() {
    let (_d, engine) = engine_for_fixture("tests/fixtures/docs", "project");
    let handler = handler_over(Some(engine));
    for (tool, args) in tool_calls() {
        let mut args = args;
        args["format"] = json!("yaml");
        let res = call(&handler, tool, args).await;
        assert_eq!(res.is_error, Some(true), "{tool}");
        assert!(text_of(&res).contains("Invalid format 'yaml'"), "{tool}");
    }
    let res = call(&handler, "init", json!({"format": "xml"})).await;
    assert_eq!(res.is_error, Some(true));
}

#[tokio::test]
async fn errors_carry_the_error_envelope_and_is_error() {
    let (_d, engine) = engine_for_fixture("tests/fixtures/docs", "project");
    let handler = handler_over(Some(engine));

    let res = call(
        &handler,
        "doc_outline",
        json!({"path": "docs/nope.md", "format": "json"}),
    )
    .await;
    assert_eq!(res.is_error, Some(true));
    let v = Value::Object(res.structured_content.clone().unwrap());
    assert_eq!(v["error"]["code"], "not_found");
    assert_eq!(parse(&res), v);
    assert!(v.get("results").is_none());

    let res = call(&handler, "query", json!({"format": "json"})).await;
    assert_eq!(res.is_error, Some(true));
    let v = parse(&res);
    assert_eq!(v["error"]["code"], "invalid_argument");
    assert_eq!(
        v["error"]["message"],
        "Error: Provide query, path, or filename"
    );

    // TOON keeps the 0.3.0 message text.
    let res = call(&handler, "query", json!({})).await;
    assert_eq!(text_of(&res), "Error: Provide query, path, or filename");
}

fn parse(res: &CallToolResult) -> Value {
    serde_json::from_str(&text_of(res)).unwrap()
}

#[tokio::test]
async fn a_missing_index_gives_typed_errors_except_for_status() {
    let handler = handler_over(None);
    for (tool, args) in tool_calls() {
        if tool == "status" {
            continue;
        }
        let toon = call(&handler, tool, args.clone()).await;
        assert_eq!(text_of(&toon), "Error: Index not loaded", "{tool}");
        assert_eq!(toon.is_error, Some(true), "{tool}");

        let mut args = args;
        args["format"] = json!("json");
        let res = call(&handler, tool, args).await;
        assert_eq!(res.is_error, Some(true), "{tool}");
        let v = parse(&res);
        assert_eq!(v["error"]["code"], "index_missing", "{tool}");
        assert_eq!(v["index"]["state"], "missing", "{tool}");
        assert!(v["index"]["commit"].is_null(), "{tool}");
        assert_eq!(Value::Object(res.structured_content.unwrap()), v);
    }

    // `status` answers, and says what to do.
    let res = call(&handler, "status", json!({"format": "json"})).await;
    assert_ne!(res.is_error, Some(true));
    let v = parse(&res);
    assert_eq!(v["index"]["state"], "missing");
    assert!(v["summary"]["next_step"]
        .as_str()
        .unwrap()
        .contains("refresh"));
    let toon = call(&handler, "status", json!({})).await;
    assert!(text_of(&toon).contains("No index found"));
}

#[tokio::test]
async fn a_stale_index_gives_index_stale_from_imports_and_the_doc_tools() {
    let engine = QueryEngine::load(std::path::Path::new(
        "tests/fixtures/compatibility/v0_2_0_index.json",
    ))
    .unwrap();
    let handler = handler_over(Some(engine));
    for (tool, args) in [
        ("imports", json!({"package": "serde"})),
        ("doc_search", json!({"query": "audit"})),
        ("api_surface", json!({})),
    ] {
        let mut args = args;
        args["format"] = json!("json");
        let res = call(&handler, tool, args).await;
        assert_eq!(res.is_error, Some(true), "{tool}");
        let v = parse(&res);
        assert_eq!(v["error"]["code"], "index_stale", "{tool}");
        assert_eq!(v["index"]["state"], "stale", "{tool}");
    }
    // Code tools keep working on an old index.
    let res = call(&handler, "status", json!({"format": "json"})).await;
    assert_eq!(parse(&res)["index"]["state"], "stale");
}

#[tokio::test]
async fn limit_caps_toon_lists_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let mut src = String::new();
    for i in 0..30 {
        src.push_str(&format!("pub fn widget_{i}() {{}}\n"));
    }
    std::fs::write(root.join("w.rs"), src).unwrap();
    let handler = handler_over(Some(build_engine(&root, dir.path())));

    let full = text_of(&call(&handler, "search", json!({"query": "widget"})).await);
    assert!(!full.contains("[limit"), "no limit, no note");
    let cut = text_of(&call(&handler, "search", json!({"query": "widget", "limit": 3})).await);
    assert!(cut.contains("[limit 3: 30 matched]"), "{cut}");
    assert_eq!(cut.matches("\nMatch: ").count(), 3);
}

#[tokio::test]
async fn max_depth_cuts_status_and_doc_outline_and_says_so() {
    let (_d, engine) = engine_for_fixture("tests/fixtures/docs", "project");
    let handler = handler_over(Some(engine));

    let res = call(
        &handler,
        "status",
        json!({"max_depth": 1, "format": "json"}),
    )
    .await;
    let v = parse(&res);
    assert_eq!(v["truncated"], true);
    let tree = &v["summary"]["tree"];
    assert!(tree["children"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c.get("children").is_none()));
    let toon = text_of(&call(&handler, "status", json!({"max_depth": 1})).await);
    assert!(toon.contains("[tree cut at depth 1]"), "{toon}");

    let toon = text_of(
        &call(
            &handler,
            "doc_outline",
            json!({"path": "docs/guide.md", "max_depth": 1}),
        )
        .await,
    );
    assert!(toon.contains("[outline cut at depth 1]"), "{toon}");
    let default = text_of(&call(&handler, "doc_outline", json!({"path": "docs/guide.md"})).await);
    assert!(!default.contains("[outline cut"));
}

#[tokio::test]
async fn modules_and_imports_have_toon_forms() {
    let (_d, engine) = engine_for_fixture(IMPORTS_FIXTURE, "project");
    let handler = handler_over(Some(engine));

    let toon = text_of(&call(&handler, "modules", json!({"max_depth": 1})).await);
    assert!(
        toon.starts_with("Resource: Modules | Prefix: . | Max Depth: 1"),
        "{toon}"
    );
    assert!(toon.contains("- js | files:"), "{toon}");

    let toon = text_of(&call(&handler, "imports", json!({"package": "@sentry/*"})).await);
    assert!(
        toon.starts_with("Resource: Imports | Package: @sentry/*"),
        "{toon}"
    );
    assert!(
        toon.contains("[@sentry/"),
        "prefix queries name each package: {toon}"
    );

    let toon = text_of(&call(&handler, "imports", json!({"package": "stripe"})).await);
    assert!(toon.contains("Files: "));
    assert!(!toon.contains("[stripe]"));
}

#[tokio::test]
async fn init_reports_the_new_commit_and_dirty_flag() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let sha = git_fixture("tests/fixtures/docs", &root);
    let out = dir.path().join("out");
    let handler = McpServerHandler::with_paths(
        root.canonicalize().unwrap(),
        out,
        Arc::new(RwLock::new(None)),
    );

    let res = call(&handler, "init", json!({"format": "json"})).await;
    assert_ne!(res.is_error, Some(true), "{}", text_of(&res));
    let v = parse(&res);
    assert_eq!(v["tool"], "init");
    assert_eq!(v["summary"]["status"], "refreshed");
    assert_eq!(v["summary"]["commit"], sha.as_str());
    assert_eq!(v["summary"]["dirty"], false);
    assert_eq!(v["index"]["state"], "ok");
    assert_eq!(v["index"]["commit"], sha.as_str());

    // The index is loaded: a tool now answers.
    let res = call(
        &handler,
        "search",
        json!({"query": "audit", "format": "json"}),
    )
    .await;
    assert!(parse(&res)["total"].as_u64().unwrap() > 0);

    let toon = call(&handler, "init", json!({})).await;
    assert!(text_of(&toon).starts_with("Index refreshed successfully."));
    assert!(text_of(&toon).contains(&format!("Commit: {sha}")));
}

#[tokio::test]
async fn every_tool_declares_format_without_an_output_schema() {
    let handler = handler_over(None);
    let tools = handler
        .handle_list_tools_request(None, Arc::new(DummyRuntime))
        .await
        .unwrap()
        .tools;
    assert_eq!(tools.len(), 15);
    for tool in tools {
        let schema = serde_json::to_value(&tool.input_schema).unwrap();
        let format = &schema["properties"]["format"];
        let description = format["description"].as_str().unwrap_or_default();
        assert!(
            description.contains("programs") && description.contains("more tokens"),
            "{}: format description {description:?}",
            tool.name
        );
        assert!(
            tool.output_schema.is_none(),
            "{}: no outputSchema",
            tool.name
        );
        // `format` is optional.
        let required = schema["required"].as_array().cloned().unwrap_or_default();
        assert!(!required.contains(&json!("format")), "{}", tool.name);
    }
}
