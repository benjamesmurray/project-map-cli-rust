use std::path::Path;
use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use rust_mcp_sdk::{McpServer as SdkMcpServer, TransportOptions, StdioTransport, ToMcpServerHandler};
use rust_mcp_sdk::mcp_server::{server_runtime, McpServerOptions, ServerHandler};
use rust_mcp_sdk::schema::{
    CallToolRequestParams, CallToolResult, InitializeResult,
    PaginatedRequestParams, ListToolsResult, ServerCapabilities, ServerCapabilitiesTools,
    Implementation, ProtocolVersion, RpcError,
};
use rust_mcp_sdk::schema::schema_utils::CallToolError;
use rust_mcp_sdk::macros::{mcp_tool, JsonSchema};
use tracing_subscriber::fmt;

use crate::error::Result;
use crate::core::query_engine::QueryEngine;
use crate::core::orchestrator::Orchestrator;
use crate::core::toon::ToonFormatter;
use crate::core::utils::render_tree;

// --- Tool Definitions ---

#[mcp_tool(
    name = "status",
    description = "Returns current workspace context and available commands."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct StatusTool {}

#[mcp_tool(
    name = "query",
    description = "Search for symbols or get file context."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct QueryTool {
    /// Search query for symbols
    pub query: Option<String>,
    /// File path to get outline
    pub path: Option<String>,
    /// Search for filenames across the tree
    pub filename: Option<String>,
}

#[mcp_tool(
    name = "blast_radius",
    description = "Identifies all components and files that depend on or import a specific symbol."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct BlastRadiusTool {
    /// File path where the symbol is defined
    pub path: String,
    /// Symbol name to check
    pub symbol: String,
}

#[mcp_tool(
    name = "plan",
    description = "Analyze the architectural impact (fan-out) of a symbol before starting a refactor."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PlanTool {
    /// Symbol name to analyze
    pub symbol: String,
}

#[mcp_tool(
    name = "search",
    description = "Search for logic using natural language keywords (e.g., 'auth', 'database')."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct SearchTool {
    /// Natural language query
    pub query: String,
}

#[mcp_tool(
    name = "fetch_symbol",
    description = "Extract raw source code for a specific class or function."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct FetchSymbolTool {
    /// File path
    pub path: String,
    /// Symbol name
    pub symbol: String,
}

#[mcp_tool(
    name = "init",
    description = "Refresh the map index after significant code changes to maintain discovery accuracy."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct InitTool {}

// --- Server Implementation ---

pub struct McpServer {
    engine: Arc<std::sync::RwLock<Option<QueryEngine>>>,
}

impl McpServer {
    pub fn new() -> Self {
        let engine = QueryEngine::load(Path::new(".project-map/latest/.project-map.json")).ok();
        Self {
            engine: Arc::new(std::sync::RwLock::new(engine)),
        }
    }

    pub fn with_engine(engine: Arc<std::sync::RwLock<Option<QueryEngine>>>) -> Self {
        Self { engine }
    }

    pub fn engine(&self) -> Arc<std::sync::RwLock<Option<QueryEngine>>> {
        Arc::clone(&self.engine)
    }

    pub async fn run(&self) -> Result<()> {
        let _ = fmt()
            .with_writer(std::io::stderr)
            .try_init();

        let server_info = InitializeResult {
            protocol_version: ProtocolVersion::V2025_11_25.into(),
            capabilities: ServerCapabilities {
                tools: Some(ServerCapabilitiesTools { list_changed: None }),
                resources: Some(rust_mcp_sdk::schema::ServerCapabilitiesResources { subscribe: None, list_changed: None }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "project-map-cli-rust".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Project Map CLI".to_string()),
                description: None,
                icons: vec![],
                website_url: None,
            },
            instructions: None,
            meta: None,
        };

        let transport = StdioTransport::new(TransportOptions::default())
            .map_err(|e| crate::error::AppError::Generic(format!("Transport error: {}", e)))?;
        let handler = self.clone_for_handler();
        
        let options = McpServerOptions {
            server_details: server_info,
            transport,
            handler: handler.to_mcp_server_handler(),
            task_store: None,
            client_task_store: None,
            message_observer: None,
        };

        let server = server_runtime::create_server(options);
        server.start().await.map_err(|e| crate::error::AppError::Generic(format!("Server error: {}", e)))?;

        Ok(())
    }

    fn clone_for_handler(&self) -> McpServerHandler {
        McpServerHandler {
            engine: Arc::clone(&self.engine),
        }
    }
}

pub struct McpServerHandler {
    engine: Arc<std::sync::RwLock<Option<QueryEngine>>>,
}

impl McpServerHandler {
    pub fn new(engine: Arc<std::sync::RwLock<Option<QueryEngine>>>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl ServerHandler for McpServerHandler {
    async fn handle_list_tools_request(
        &self,
        _params: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn SdkMcpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                StatusTool::tool(),
                QueryTool::tool(),
                BlastRadiusTool::tool(),
                PlanTool::tool(),
                SearchTool::tool(),
                FetchSymbolTool::tool(),
                InitTool::tool(),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn handle_list_resources_request(
        &self,
        _params: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn SdkMcpServer>,
    ) -> std::result::Result<rust_mcp_sdk::schema::ListResourcesResult, RpcError> {
        use rust_mcp_sdk::schema::{ListResourcesResult, Resource};
        let status_res = Resource {
            uri: "project-map://status".to_string(),
            name: "Project Map Status".to_string(),
            title: Some("Project Map Status".to_string()),
            description: Some("Index status, symbol count, file count, and last indexed timestamp".to_string()),
            mime_type: Some("application/json".to_string()),
            icons: vec![],
            size: None,
            annotations: None,
            meta: None,
        };
        let tree_res = Resource {
            uri: "project-map://tree".to_string(),
            name: "Project Map Tree".to_string(),
            title: Some("Project Map Tree".to_string()),
            description: Some("High-level project file structure and architectural pulse".to_string()),
            mime_type: Some("text/plain".to_string()),
            icons: vec![],
            size: None,
            annotations: None,
            meta: None,
        };
        Ok(ListResourcesResult {
            resources: vec![status_res, tree_res],
            next_cursor: None,
            meta: None,
        })
    }

    async fn handle_read_resource_request(
        &self,
        params: rust_mcp_sdk::schema::ReadResourceRequestParams,
        _runtime: Arc<dyn SdkMcpServer>,
    ) -> std::result::Result<rust_mcp_sdk::schema::ReadResourceResult, RpcError> {
        use rust_mcp_sdk::schema::{ReadResourceResult, ReadResourceContent, TextResourceContents};
        let index_path = Path::new(".project-map/latest/.project-map.json");
        let content = match params.uri.as_str() {
            "project-map://status" => {
                let status_json = if let Some(ref engine) = *self.engine.read().unwrap() {
                    let symbol_count = engine.get_symbol_count();
                    let file_count = engine.get_file_count();
                    let last_indexed = QueryEngine::get_last_indexed_time(index_path);
                    serde_json::json!({
                        "is_ready": true,
                        "index_path": index_path.to_string_lossy(),
                        "symbol_count": symbol_count,
                        "file_count": file_count,
                        "last_indexed": last_indexed
                    })
                } else {
                    serde_json::json!({
                        "is_ready": false,
                        "index_path": null,
                        "symbol_count": 0,
                        "file_count": 0,
                        "last_indexed": null
                    })
                };
                ReadResourceContent::TextResourceContents(TextResourceContents {
                    uri: "project-map://status".to_string(),
                    mime_type: Some("application/json".to_string()),
                    text: status_json.to_string(),
                    meta: None,
                })
            }
            "project-map://tree" => {
                let tree_text = if let Some(ref engine) = *self.engine.read().unwrap() {
                    let paths = engine.get_all_file_paths();
                    render_tree(&paths, 3)
                } else {
                    "Index not initialized. Run 'project-map init' or pass --watch to build index.".to_string()
                };
                ReadResourceContent::TextResourceContents(TextResourceContents {
                    uri: "project-map://tree".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: tree_text,
                    meta: None,
                })
            }
            _ => {
                return Err(RpcError::invalid_params());
            }
        };
        Ok(ReadResourceResult {
            contents: vec![content],
            meta: None,
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn SdkMcpServer>,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let arguments = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let text = match params.name.as_str() {
            "status" => {
                let (is_ready, tree, active_features) = if let Some(ref engine) = *self.engine.read().unwrap() {
                    let paths = engine.get_all_file_paths();
                    let tree = render_tree(&paths, 3);
                    
                    let mut active = Vec::new();
                    if let Ok(entries) = std::fs::read_dir("projects/active") {
                        for entry in entries.flatten() {
                            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                                active.push(entry.file_name().to_string_lossy().into_owned());
                            }
                        }
                    }
                    (true, Some(tree), active)
                } else {
                    (false, None, Vec::new())
                };

                ToonFormatter::format_status(is_ready, Some(".project-map/latest/.project-map.json"), tree.as_deref(), &active_features)
            }
            "query" => {
                let args: QueryTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    if let Some(q) = args.query {
                        let matches = engine.find_symbols(&q);
                        ToonFormatter::format_symbols(&q, &matches)
                    } else if let Some(p) = args.path {
                        let symbols = engine.get_file_outline(&p);
                        ToonFormatter::format_file_context(&p, &symbols)
                    } else if let Some(f) = args.filename {
                        let matches = engine.find_files(&f);
                        ToonFormatter::format_file_matches(&f, &matches)
                    } else {
                        "Error: Provide query, path, or filename".to_string()
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "blast_radius" => {
                let args: BlastRadiusTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let results = engine.check_blast_radius(&args.path, &args.symbol);
                    ToonFormatter::format_blast_radius(&args.path, &args.symbol, &results)
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "plan" => {
                let args: PlanTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let impact = engine.analyze_impact(&args.symbol);
                    ToonFormatter::format_impact_analysis(&args.symbol, &impact)
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "search" => {
                let args: SearchTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let matches = engine.find_symbols(&args.query);
                    ToonFormatter::format_symbols(&args.query, &matches)
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "fetch_symbol" => {
                let args: FetchSymbolTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    if let Some(node) = engine.find_symbol_in_path(&args.path, &args.symbol) {
                        if let Ok(content) = std::fs::read_to_string(&node.path) {
                            let bytes = content.as_bytes();
                            if node.start_byte < bytes.len() && node.end_byte <= bytes.len() {
                                let content_str = String::from_utf8_lossy(&bytes[node.start_byte..node.end_byte]);
                                ToonFormatter::format_fetch_result(&args.path, &args.symbol, Some(&content_str))
                            } else {
                                "Error: Byte range out of bounds".to_string()
                            }
                        } else {
                            "Error: Could not read file".to_string()
                        }
                    } else {
                        ToonFormatter::format_fetch_result(&args.path, &args.symbol, None)
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "init" => {
                let mut orch = Orchestrator::new();
                let _ = orch.scaffold_if_empty(Path::new("."));
                if orch.build_index(Path::new(".")).is_ok() && orch.save_index_versioned(Path::new(".project-map")).is_ok() {
                    let new_engine = QueryEngine::load(Path::new(".project-map/latest/.project-map.json")).ok();
                    *self.engine.write().unwrap() = new_engine;
                    "Index refreshed successfully.".to_string()
                } else {
                    "Failed to refresh index.".to_string()
                }
            }

            _ => return Err(CallToolError(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Unknown tool")))),
        };

        Ok(CallToolResult::text_content(vec![text.into()]))
    }
}

