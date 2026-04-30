use std::path::Path;
use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use rust_mcp_sdk::{McpServer as SdkMcpServer, TransportOptions, StdioTransport};
use rust_mcp_sdk::mcp_server::{server_runtime, ServerHandler};
use rust_mcp_sdk::schema::{
    CallToolRequest, CallToolResult, InitializeResult,
    ListToolsRequest, ListToolsResult, ServerCapabilities, ServerCapabilitiesTools,
    Implementation, ProtocolVersion, RpcError,
};
use rust_mcp_sdk::schema::schema_utils::CallToolError;
use rust_mcp_sdk::macros::{mcp_tool, JsonSchema};
use tracing_subscriber::fmt;

use crate::error::Result;
use crate::core::query_engine::QueryEngine;
use crate::core::orchestrator::Orchestrator;

// --- Tool Definitions ---

#[mcp_tool(
    name = "pm_status",
    description = "Returns current workspace context and available commands."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmStatusTool {}

#[mcp_tool(
    name = "pm_query",
    description = "Search for symbols or get file context."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmQueryTool {
    /// Search query for symbols
    pub query: Option<String>,
    /// File path to get outline
    pub path: Option<String>,
}

#[mcp_tool(
    name = "pm_check_blast_radius",
    description = "Identifies all components and files that depend on or import a specific symbol."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmCheckBlastRadiusTool {
    /// File path where the symbol is defined
    pub path: String,
    /// Symbol name to check
    pub symbol: String,
}

#[mcp_tool(
    name = "pm_plan",
    description = "Analyze the architectural impact (fan-out) of a symbol before starting a refactor."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmPlanTool {
    /// Symbol name to analyze
    pub symbol: String,
}

#[mcp_tool(
    name = "pm_semantic_search",
    description = "Search for logic using natural language keywords (e.g., 'auth', 'database')."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmSemanticSearchTool {
    /// Natural language query
    pub query: String,
}

#[mcp_tool(
    name = "pm_fetch_symbol",
    description = "Extract raw source code for a specific class or function."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmFetchSymbolTool {
    /// File path
    pub path: String,
    /// Symbol name
    pub symbol: String,
}

#[mcp_tool(
    name = "pm_init",
    description = "Refresh the map index after significant code changes to maintain discovery accuracy."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PmInitTool {}

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

    pub async fn run(&self) -> Result<()> {
        let _ = fmt()
            .with_writer(std::io::stderr)
            .try_init();

        let server_info = InitializeResult {
            protocol_version: ProtocolVersion::V2024_11_05.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ServerCapabilitiesTools { list_changed: None }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "project-map-cli-rust".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Project Map CLI".to_string()),
            },
            instructions: None,
            meta: None,
        };

        let transport = StdioTransport::new(TransportOptions::default())
            .map_err(|e| crate::error::AppError::Generic(format!("Transport error: {}", e)))?;
        let handler = self.clone_for_handler();
        
        let server = server_runtime::create_server(server_info, transport, handler);
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

#[async_trait]
impl ServerHandler for McpServerHandler {
    async fn handle_list_tools_request(
        &self,
        _request: ListToolsRequest,
        _runtime: &dyn SdkMcpServer,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                PmStatusTool::tool(),
                PmQueryTool::tool(),
                PmCheckBlastRadiusTool::tool(),
                PmPlanTool::tool(),
                PmSemanticSearchTool::tool(),
                PmFetchSymbolTool::tool(),
                PmInitTool::tool(),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn on_server_started(&self, _runtime: &dyn SdkMcpServer) {
        // Silence the default "Server started successfully" message which can corrupt stdout
    }

    async fn handle_call_tool_request(
        &self,
        request: CallToolRequest,
        _runtime: &dyn SdkMcpServer,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let arguments = serde_json::Value::Object(request.params.arguments.unwrap_or_default());
        let text = match request.params.name.as_str() {
            "pm_status" => {
                if self.engine.read().unwrap().is_some() {
                    "Status: System healthy. Index is present.".to_string()
                } else {
                    "Status: Index missing. Run project-map build.".to_string()
                }
            }
            "pm_query" => {
                let args: PmQueryTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    if let Some(q) = args.query {
                        let matches = engine.find_symbols(&q);
                        format!("Matches: {}", matches.len())
                    } else if let Some(p) = args.path {
                        let symbols = engine.get_file_outline(&p);
                        format!("Symbols in {}: {}", p, symbols.len())
                    } else {
                        "Error: Provide query or path".to_string()
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_check_blast_radius" => {
                let args: PmCheckBlastRadiusTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let results = engine.check_blast_radius(&args.path, &args.symbol);

                    if results.is_empty() {
                        "No dependent components found.".to_string()
                    } else {
                        let mut unique_files = std::collections::HashSet::new();
                        for r in &results { unique_files.insert(&r.path); }
                        format!("Blast Radius for {}:\n- Total Impacted Nodes: {}\n- Unique Files: {}\n(Top 5: {})", 
                            args.symbol, results.len(), unique_files.len(),
                            results.iter().take(5).map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "))
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_plan" => {
                let args: PmPlanTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let impact = engine.analyze_impact(&args.symbol);
                    let blast = engine.check_blast_radius("", &args.symbol);

                    let mut unique_blast = std::collections::HashSet::new();
                    for r in &blast { unique_blast.insert(&r.path); }

                    format!("Architectural Plan for {}:\n- Fan-out (Dependencies): {} nodes\n- Fan-in (Dependents): {} nodes across {} files.", 
                        args.symbol, impact.len(), blast.len(), unique_blast.len())
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_semantic_search" => {
                let args: PmSemanticSearchTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let matches = engine.find_symbols(&args.query);
                    let mut result = format!("Semantic Search Results ({}):", matches.len());
                    for m in matches.iter().take(15) {
                        result.push_str(&format!("\n- {}: {}", m.path, m.name));
                    }
                    result
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_fetch_symbol" => {
                let args: PmFetchSymbolTool = serde_json::from_value(arguments)
                    .map_err(|e| CallToolError(Box::new(e)))?;
                
                if let Some(ref engine) = *self.engine.read().unwrap() {
                    if let Some(node) = engine.find_symbol_in_path(&args.path, &args.symbol) {
                        if let Ok(content) = std::fs::read_to_string(&node.path) {
                            let bytes = content.as_bytes();
                            if node.start_byte < bytes.len() && node.end_byte <= bytes.len() {
                                String::from_utf8_lossy(&bytes[node.start_byte..node.end_byte]).to_string()
                            } else {
                                "Error: Byte range out of bounds".to_string()
                            }
                        } else {
                            "Error: Could not read file".to_string()
                        }
                    } else {
                        "Error: Symbol not found".to_string()
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_init" => {
                let mut orch = Orchestrator::new();
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
