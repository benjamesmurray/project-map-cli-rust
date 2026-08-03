use std::path::Path;
use std::sync::Arc;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::mcp::server::McpServerHandler;
use rust_mcp_sdk::mcp_server::ServerHandler;
use rust_mcp_sdk::schema::{CallToolRequestParams, ReadResourceRequestParams, ReadResourceContent};

struct DummyRuntime;
#[async_trait::async_trait]
impl rust_mcp_sdk::McpServer for DummyRuntime {
    async fn start(self: Arc<Self>) -> rust_mcp_sdk::error::SdkResult<()> { todo!() }
    async fn set_client_details(&self, _: rust_mcp_sdk::schema::InitializeRequestParams) -> rust_mcp_sdk::error::SdkResult<()> { todo!() }
    fn server_info(&self) -> &rust_mcp_sdk::schema::InitializeResult { todo!() }
    fn client_info(&self) -> Option<rust_mcp_sdk::schema::InitializeRequestParams> { todo!() }
    async fn auth_info<'a>(&'a self) -> tokio::sync::RwLockReadGuard<'a, Option<rust_mcp_sdk::auth::AuthInfo>> { todo!() }
    async fn auth_info_cloned(&self) -> Option<rust_mcp_sdk::auth::AuthInfo> { todo!() }
    async fn update_auth_info(&self, _: Option<rust_mcp_sdk::auth::AuthInfo>) { todo!() }
    async fn wait_for_initialization(&self) { todo!() }
    fn task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ServerTaskStore>> { todo!() }
    fn client_task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ClientTaskStore>> { todo!() }
    async fn stderr_message(&self, _: String) -> rust_mcp_sdk::error::SdkResult<()> { todo!() }
    fn session_id(&self) -> Option<String> { todo!() }
    async fn send(&self, _: rust_mcp_sdk::schema::schema_utils::MessageFromServer, _: Option<rust_mcp_sdk::schema::RequestId>, _: Option<std::time::Duration>) -> rust_mcp_sdk::error::SdkResult<Option<rust_mcp_sdk::schema::schema_utils::ClientMessage>> { todo!() }
    async fn send_batch(&self, _: Vec<rust_mcp_sdk::schema::schema_utils::ServerMessage>, _: Option<std::time::Duration>) -> rust_mcp_sdk::error::SdkResult<Option<Vec<rust_mcp_sdk::schema::schema_utils::ClientMessage>>> { todo!() }
}

fn create_test_handler(test_name: &str) -> McpServerHandler {
    let root = Path::new("tests/fixtures/python");
    let out = std::env::temp_dir().join(format!("test-mcp-{}.json", test_name));
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(&out).expect("Failed to save index");
    
    let engine = QueryEngine::load(&out).expect("Failed to load index");
    std::fs::remove_file(out).ok();
    
    McpServerHandler::new(Arc::new(std::sync::RwLock::new(Some(engine))))
}

#[tokio::test]
async fn test_mcp_tool_list_names() {
    let handler = create_test_handler("list_names");
    let runtime = Arc::new(DummyRuntime);
    let result = handler.handle_list_tools_request(None, runtime).await.unwrap();
    let names: Vec<String> = result.tools.into_iter().map(|t| t.name).collect();
    
    assert_eq!(
        names,
        vec!["status", "query", "blast_radius", "plan", "search", "fetch_symbol", "init"]
    );
}

#[tokio::test]
async fn test_mcp_resources_list() {
    let handler = create_test_handler("res_list");
    let runtime = Arc::new(DummyRuntime);
    let result = handler.handle_list_resources_request(None, runtime).await.unwrap();
    let uris: Vec<String> = result.resources.into_iter().map(|r| r.uri).collect();
    
    assert_eq!(
        uris,
        vec!["project-map://status", "project-map://tree"]
    );
}

#[tokio::test]
async fn test_mcp_resources_read_status() {
    let handler = create_test_handler("res_status");
    let runtime = Arc::new(DummyRuntime);
    
    let params = ReadResourceRequestParams {
        uri: "project-map://status".to_string(),
        meta: None,
    };
    let result = handler.handle_read_resource_request(params, runtime).await.unwrap();
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
    let result = handler.handle_read_resource_request(params, runtime).await.unwrap();
    assert_eq!(result.contents.len(), 1);
    
    if let ReadResourceContent::TextResourceContents(trc) = &result.contents[0] {
        assert_eq!(trc.uri, "project-map://tree");
        assert_eq!(trc.mime_type.as_deref(), Some("text/plain"));
        assert!(!trc.text.is_empty(), "Tree rendering output should not be empty");
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
    let res = handler.handle_call_tool_request(call_params, runtime.clone()).await.unwrap();
    assert!(!res.content.is_empty());

    // 2. Call search tool
    let call_params = CallToolRequestParams {
        name: "search".to_string(),
        arguments: Some(serde_json::json!({ "query": "hello" }).as_object().unwrap().clone()),
        meta: None,
        task: None,
    };
    let res = handler.handle_call_tool_request(call_params, runtime.clone()).await.unwrap();
    assert!(!res.content.is_empty());
}
