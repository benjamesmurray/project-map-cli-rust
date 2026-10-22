use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::mcp::server::McpServerHandler;
use rust_mcp_sdk::mcp_server::ServerHandler;
use rust_mcp_sdk::schema::{CallToolRequestParams, ReadResourceContent, ReadResourceRequestParams};
use std::path::Path;
use std::sync::Arc;

struct DummyRuntime;
#[async_trait::async_trait]
impl rust_mcp_sdk::McpServer for DummyRuntime {
    async fn start(self: Arc<Self>) -> rust_mcp_sdk::error::SdkResult<()> {
        todo!()
    }
    async fn set_client_details(
        &self,
        _: rust_mcp_sdk::schema::InitializeRequestParams,
    ) -> rust_mcp_sdk::error::SdkResult<()> {
        todo!()
    }
    fn server_info(&self) -> &rust_mcp_sdk::schema::InitializeResult {
        todo!()
    }
    fn client_info(&self) -> Option<rust_mcp_sdk::schema::InitializeRequestParams> {
        todo!()
    }
    async fn auth_info<'a>(
        &'a self,
    ) -> tokio::sync::RwLockReadGuard<'a, Option<rust_mcp_sdk::auth::AuthInfo>> {
        todo!()
    }
    async fn auth_info_cloned(&self) -> Option<rust_mcp_sdk::auth::AuthInfo> {
        todo!()
    }
    async fn update_auth_info(&self, _: Option<rust_mcp_sdk::auth::AuthInfo>) {
        todo!()
    }
    async fn wait_for_initialization(&self) {
        todo!()
    }
    fn task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ServerTaskStore>> {
        todo!()
    }
    fn client_task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ClientTaskStore>> {
        todo!()
    }
    async fn stderr_message(&self, _: String) -> rust_mcp_sdk::error::SdkResult<()> {
        todo!()
    }
    fn session_id(&self) -> Option<String> {
        todo!()
    }
    async fn send(
        &self,
        _: rust_mcp_sdk::schema::schema_utils::MessageFromServer,
        _: Option<rust_mcp_sdk::schema::RequestId>,
        _: Option<std::time::Duration>,
    ) -> rust_mcp_sdk::error::SdkResult<Option<rust_mcp_sdk::schema::schema_utils::ClientMessage>>
    {
        todo!()
    }
    async fn send_batch(
        &self,
        _: Vec<rust_mcp_sdk::schema::schema_utils::ServerMessage>,
        _: Option<std::time::Duration>,
    ) -> rust_mcp_sdk::error::SdkResult<
        Option<Vec<rust_mcp_sdk::schema::schema_utils::ClientMessage>>,
    > {
        todo!()
    }
}

fn create_test_handler(test_name: &str) -> McpServerHandler {
    let root = Path::new("tests/fixtures/python");
    let out = std::env::temp_dir().join(format!("test-mcp-{}.json", test_name));

    let mut orch = Orchestrator::new();
    let out_dir = out.parent().unwrap_or_else(|| Path::new("."));
    orch.build_index(root, out_dir)
        .expect("Failed to build index");
    orch.save_index(&out).expect("Failed to save index");

    let engine = QueryEngine::load(&out).expect("Failed to load index");
    std::fs::remove_file(out).ok();

    McpServerHandler::new(Arc::new(std::sync::RwLock::new(Some(engine))))
}

#[tokio::test]
async fn test_mcp_tool_list_names() {
    let handler = create_test_handler("list_names");
    let runtime = Arc::new(DummyRuntime);
    let result = handler
        .handle_list_tools_request(None, runtime)
        .await
        .unwrap();
    let names: Vec<String> = result.tools.into_iter().map(|t| t.name).collect();

    assert_eq!(
        names,
        vec![
            "status",
            "query",
            "blast_radius",
            "plan",
            "search",
            "fetch_symbol",
            "init",
            "doc_search",
            "doc_outline",
            "doc_section",
            "doc_links",
            "doc_terms",
            "api_surface"
        ]
    );
}

#[tokio::test]
async fn test_mcp_resources_list() {
    let handler = create_test_handler("res_list");
    let runtime = Arc::new(DummyRuntime);
    let result = handler
        .handle_list_resources_request(None, runtime)
        .await
        .unwrap();
    let uris: Vec<String> = result.resources.into_iter().map(|r| r.uri).collect();

    assert_eq!(uris, vec!["project-map://status", "project-map://tree"]);
}

#[tokio::test]
async fn test_mcp_resources_read_status() {
    let handler = create_test_handler("res_status");
    let runtime = Arc::new(DummyRuntime);

    let params = ReadResourceRequestParams {
        uri: "project-map://status".to_string(),
        meta: None,
    };
    let result = handler
        .handle_read_resource_request(params, runtime)
        .await
        .unwrap();
    assert_eq!(result.contents.len(), 1);

    if let ReadResourceContent::TextResourceContents(trc) = &result.contents[0] {
        assert_eq!(trc.uri, "project-map://status");
        assert_eq!(trc.mime_type.as_deref(), Some("application/json"));

        let json: serde_json::Value = serde_json::from_str(&trc.text).unwrap();
        assert_eq!(json["is_ready"], true);
        assert!(json["symbol_count"].as_u64().unwrap() > 0);
        assert!(json["file_count"].as_u64().unwrap() > 0);
    } else {
        panic!("Expected TextResourceContents");
    }
}

#[tokio::test]
async fn test_mcp_resources_read_tree() {
    let handler = create_test_handler("res_tree");
    let runtime = Arc::new(DummyRuntime);

    let params = ReadResourceRequestParams {
        uri: "project-map://tree".to_string(),
        meta: None,
    };
    let result = handler
        .handle_read_resource_request(params, runtime)
        .await
        .unwrap();
    assert_eq!(result.contents.len(), 1);

    if let ReadResourceContent::TextResourceContents(trc) = &result.contents[0] {
        assert_eq!(trc.uri, "project-map://tree");
        assert_eq!(trc.mime_type.as_deref(), Some("text/plain"));
        assert!(
            !trc.text.is_empty(),
            "Tree rendering output should not be empty"
        );
    } else {
        panic!("Expected TextResourceContents");
    }
}

#[tokio::test]
async fn test_mcp_resources_read_invalid_uri() {
    let handler = create_test_handler("res_invalid");
    let runtime = Arc::new(DummyRuntime);

    let params = ReadResourceRequestParams {
        uri: "project-map://unknown".to_string(),
        meta: None,
    };
    let result = handler.handle_read_resource_request(params, runtime).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mcp_tool_execution_renamed() {
    let handler = create_test_handler("exec_renamed");
    let runtime = Arc::new(DummyRuntime);

    // 1. Call status tool
    let call_params = CallToolRequestParams {
        name: "status".to_string(),
        arguments: None,
        meta: None,
        task: None,
    };
    let res = handler
        .handle_call_tool_request(call_params, runtime.clone())
        .await
        .unwrap();
    assert!(!res.content.is_empty());

    // 2. Call search tool
    let call_params = CallToolRequestParams {
        name: "search".to_string(),
        arguments: Some(
            serde_json::json!({ "query": "hello" })
                .as_object()
                .unwrap()
                .clone(),
        ),
        meta: None,
        task: None,
    };
    let res = handler
        .handle_call_tool_request(call_params, runtime.clone())
        .await
        .unwrap();
    assert!(!res.content.is_empty());
}

// --- Doc tools (Verification 4) ---

fn docs_handler(index: &Path) -> McpServerHandler {
    let engine = QueryEngine::load(index).expect("Failed to load index");
    McpServerHandler::new(Arc::new(std::sync::RwLock::new(Some(engine))))
}

/// A docs-fixture index built with an absolute root (paths are root-relative).
fn docs_index(test_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("test-mcp-docs-{}", test_name));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("project");
    copy_dir(Path::new("tests/fixtures/docs"), &root);
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &dir.join("out")).unwrap();
    let index = dir.join("index.json");
    orch.save_index(&index).unwrap();
    (dir, index)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

async fn call(handler: &McpServerHandler, name: &str, args: serde_json::Value) -> String {
    let params = CallToolRequestParams {
        name: name.to_string(),
        arguments: args.as_object().cloned(),
        meta: None,
        task: None,
    };
    let res = handler
        .handle_call_tool_request(params, Arc::new(DummyRuntime))
        .await
        .unwrap();
    serde_json::to_value(&res.content[0]).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_mcp_doc_tools_return_the_formatter_output() {
    use project_map_cli_rust::core::toon::{ToonFormatter, DEFAULT_MAX_CHARS};
    let (dir, index) = docs_index("outputs");
    let handler = docs_handler(&index);
    let engine = QueryEngine::load(&index).unwrap();

    let got = call(
        &handler,
        "doc_search",
        serde_json::json!({"query": "audit log"}),
    )
    .await;
    let want = ToonFormatter::format_doc_search(
        "audit log",
        &engine.search_docs("audit log", 10).unwrap(),
        DEFAULT_MAX_CHARS,
    );
    assert_eq!(got, want);
    assert!(got.contains("audit"), "{got}");

    let got = call(
        &handler,
        "doc_search",
        serde_json::json!({"query": "audit log", "limit": 1}),
    )
    .await;
    let want = ToonFormatter::format_doc_search(
        "audit log",
        &engine.search_docs("audit log", 1).unwrap(),
        DEFAULT_MAX_CHARS,
    );
    assert_eq!(got, want);

    let got = call(
        &handler,
        "doc_outline",
        serde_json::json!({"path": "docs/guide.md"}),
    )
    .await;
    let want = ToonFormatter::format_doc_outline(
        &engine.doc_outline("docs/guide.md").unwrap(),
        DEFAULT_MAX_CHARS,
    );
    assert_eq!(got, want);

    let got = call(
        &handler,
        "doc_section",
        serde_json::json!({"path": "docs/guide.md", "anchor": "usage"}),
    )
    .await;
    let want = ToonFormatter::format_doc_section(
        &engine.doc_section("docs/guide.md", "usage").unwrap(),
        DEFAULT_MAX_CHARS,
    );
    assert_eq!(got, want);

    let got = call(
        &handler,
        "doc_links",
        serde_json::json!({"path": "src/audit/log.rs", "symbol": "AuditLog"}),
    )
    .await;
    let want = ToonFormatter::format_doc_links(
        &engine
            .doc_links("src/audit/log.rs", Some("AuditLog"))
            .unwrap(),
        DEFAULT_MAX_CHARS,
    );
    assert_eq!(got, want);

    let got = call(&handler, "doc_terms", serde_json::json!({})).await;
    let want =
        ToonFormatter::format_doc_terms(&engine.doc_terms(None, 30).unwrap(), DEFAULT_MAX_CHARS);
    assert_eq!(got, want);

    let got = call(&handler, "doc_terms", serde_json::json!({"limit": 2})).await;
    let want =
        ToonFormatter::format_doc_terms(&engine.doc_terms(None, 2).unwrap(), DEFAULT_MAX_CHARS);
    assert_eq!(got, want);

    let got = call(&handler, "api_surface", serde_json::json!({})).await;
    let want =
        ToonFormatter::format_api_surface(&engine.api_surface(None).unwrap(), DEFAULT_MAX_CHARS);
    assert_eq!(got, want);
    assert!(!got.is_empty());

    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn test_mcp_doc_section_truncates_with_a_small_budget() {
    let (dir, index) = docs_index("truncate");
    let handler = docs_handler(&index);
    let args = serde_json::json!({"path": "docs/guide.md", "anchor": "usage", "max_chars": 40});
    let small = call(&handler, "doc_section", args).await;
    let full = call(
        &handler,
        "doc_section",
        serde_json::json!({"path": "docs/guide.md", "anchor": "usage"}),
    )
    .await;
    assert!(small.to_lowercase().contains("truncated"), "{small}");
    assert!(!full.to_lowercase().contains("truncated"), "{full}");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn test_mcp_doc_tools_report_errors_as_text() {
    let (dir, index) = docs_index("errors");
    let handler = docs_handler(&index);
    let out = call(
        &handler,
        "doc_outline",
        serde_json::json!({"path": "nope.md"}),
    )
    .await;
    assert!(!out.is_empty());
    let out = call(
        &handler,
        "doc_section",
        serde_json::json!({"path": "docs/guide.md", "anchor": "no-such-anchor"}),
    )
    .await;
    assert!(
        out.contains("no-such-anchor") || out.to_lowercase().contains("anchor"),
        "{out}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn test_mcp_doc_tools_return_the_stale_message_on_a_v2_index() {
    use project_map_cli_rust::core::graph::STALE_INDEX_MESSAGE;
    let handler = docs_handler(Path::new("tests/fixtures/compatibility/v0_2_0_index.json"));
    for (tool, args) in [
        ("doc_search", serde_json::json!({"query": "audit"})),
        ("doc_outline", serde_json::json!({"path": "README.md"})),
        (
            "doc_section",
            serde_json::json!({"path": "README.md", "anchor": "intro"}),
        ),
        ("doc_links", serde_json::json!({"path": "src/main.rs"})),
        ("doc_terms", serde_json::json!({})),
        ("api_surface", serde_json::json!({})),
    ] {
        assert_eq!(
            call(&handler, tool, args).await,
            STALE_INDEX_MESSAGE,
            "{tool}"
        );
    }
}

#[tokio::test]
async fn test_mcp_doc_tools_without_an_index() {
    let handler = McpServerHandler::new(Arc::new(std::sync::RwLock::new(None)));
    let out = call(&handler, "doc_search", serde_json::json!({"query": "x"})).await;
    assert_eq!(out, "Error: Index not loaded");
}

/// Use-case phrase each tool description must contain (Verification 6).
const USE_CASE_PHRASES: [(&str, &str); 13] = [
    ("status", "before grepping"),
    ("query", "outline"),
    ("blast_radius", "grep cannot follow imports"),
    ("plan", "before a refactor"),
    ("search", "code-only"),
    ("fetch_symbol", "without reading the whole file"),
    ("init", "after significant code changes"),
    ("doc_search", "instead of grepping"),
    ("doc_outline", "heading tree"),
    ("doc_section", "one section"),
    ("doc_links", "documentation that mentions"),
    ("doc_terms", "domain vocabulary"),
    ("api_surface", "OpenAPI"),
];

#[tokio::test]
async fn test_mcp_every_tool_description_names_its_use_case() {
    let handler = create_test_handler("descriptions");
    let result = handler
        .handle_list_tools_request(None, Arc::new(DummyRuntime))
        .await
        .unwrap();
    assert_eq!(result.tools.len(), 13);
    for (name, phrase) in USE_CASE_PHRASES {
        let tool = result.tools.iter().find(|t| t.name == name).unwrap();
        let description = tool.description.as_deref().unwrap_or_default();
        assert!(!description.is_empty(), "{name}: empty description");
        assert!(
            description.contains(phrase),
            "{name}: description {description:?} lacks {phrase:?}"
        );
    }
    let search = result.tools.iter().find(|t| t.name == "search").unwrap();
    assert!(search
        .description
        .as_deref()
        .unwrap()
        .contains("doc_search"));
}

#[tokio::test]
async fn test_mcp_existing_tool_schemas_are_unchanged() {
    let handler = create_test_handler("schemas");
    let result = handler
        .handle_list_tools_request(None, Arc::new(DummyRuntime))
        .await
        .unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/mcp_tool_schemas.json").unwrap(),
    )
    .unwrap();
    let snapshot = snapshot.as_object().unwrap();
    assert_eq!(snapshot.len(), 7);
    for (name, schema) in snapshot {
        let tool = result.tools.iter().find(|t| &t.name == name).unwrap();
        assert_eq!(
            &serde_json::to_value(&tool.input_schema).unwrap(),
            schema,
            "{name}"
        );
    }
}

// --- CLI subcommands print what the MCP tools return (task 6.3) ---

fn run_cli(args: &[&str]) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
        .args(args)
        .output()
        .expect("failed to run the CLI");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[tokio::test]
async fn test_cli_doc_subcommands_match_the_mcp_tools() {
    let (dir, index) = docs_index("cli");
    let handler = docs_handler(&index);
    // Build the index the CLI reads with the binary itself.
    let root = dir.join("project").canonicalize().unwrap();
    let out = dir.join("cli-out");
    run_cli(&[
        "build",
        "--root",
        root.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    let idx = out.to_str().unwrap();

    let cases: Vec<(Vec<&str>, &str, serde_json::Value)> = vec![
        (
            vec!["doc", "search", "audit log"],
            "doc_search",
            serde_json::json!({"query": "audit log"}),
        ),
        (
            vec!["doc", "search", "audit log", "--limit", "1"],
            "doc_search",
            serde_json::json!({"query": "audit log", "limit": 1}),
        ),
        (
            vec!["doc", "outline", "docs/guide.md"],
            "doc_outline",
            serde_json::json!({"path": "docs/guide.md"}),
        ),
        (
            vec!["doc", "section", "docs/guide.md", "usage"],
            "doc_section",
            serde_json::json!({"path": "docs/guide.md", "anchor": "usage"}),
        ),
        (
            vec![
                "doc",
                "section",
                "docs/guide.md",
                "usage",
                "--max-chars",
                "40",
            ],
            "doc_section",
            serde_json::json!({"path": "docs/guide.md", "anchor": "usage", "max_chars": 40}),
        ),
        (
            vec!["doc", "links", "src/audit/log.rs", "--symbol", "AuditLog"],
            "doc_links",
            serde_json::json!({"path": "src/audit/log.rs", "symbol": "AuditLog"}),
        ),
        (vec!["doc", "terms"], "doc_terms", serde_json::json!({})),
        (
            vec!["doc", "terms", "--limit", "2"],
            "doc_terms",
            serde_json::json!({"limit": 2}),
        ),
        (vec!["api"], "api_surface", serde_json::json!({})),
        (
            vec!["doc", "outline", "nope.md"],
            "doc_outline",
            serde_json::json!({"path": "nope.md"}),
        ),
    ];
    for (cli_args, tool, args) in cases {
        let mut full = cli_args.clone();
        full.extend(["--index", idx]);
        let cli = run_cli(&full);
        let mcp = call(&handler, tool, args).await;
        assert_eq!(cli.trim_end(), mcp.trim_end(), "{cli_args:?}");
    }
    std::fs::remove_dir_all(dir).ok();
}
