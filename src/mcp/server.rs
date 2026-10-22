use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use rust_mcp_sdk::macros::{mcp_tool, JsonSchema};
use rust_mcp_sdk::mcp_server::{server_runtime, McpServerOptions, ServerHandler};
use rust_mcp_sdk::schema::schema_utils::{CallToolError, CustomRequest};
use rust_mcp_sdk::schema::{
    CallToolRequestParams, CallToolResult, GenericResult, Implementation, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, RpcError, ServerCapabilities,
    ServerCapabilitiesTools,
};
use rust_mcp_sdk::{
    McpServer as SdkMcpServer, StdioTransport, ToMcpServerHandler, TransportOptions,
};
use tracing_subscriber::fmt;

use crate::core::orchestrator::rebuild_and_publish;
use crate::core::query_engine::QueryEngine;
use crate::core::toon::{ToonFormatter, DEFAULT_MAX_CHARS};
use crate::core::utils::{get_active_features, normalize_rel_path, render_tree};
use crate::error::Result;

const DEFAULT_DOC_LIMIT: usize = 10;
const DEFAULT_TERMS_LIMIT: usize = 30;

// --- Tool Definitions ---

#[mcp_tool(
    name = "status",
    description = "Orient in the workspace before grepping: index health, the project tree, active spec features and doc counts. Start here to see what the map covers."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct StatusTool {}

#[mcp_tool(
    name = "query",
    description = "Find where a symbol is defined, get a file's outline (every class and function with line numbers), or find files by name. Faster and more precise than grepping for definitions."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct QueryTool {
    /// Search query for symbols
    pub query: Option<String>,
    /// File path to get outline
    pub path: Option<String>,
    /// Search for filenames across the tree
    pub filename: Option<String>,
    /// Number of context lines for snippet preview (defaults to 3)
    pub preview: Option<u32>,
}

#[mcp_tool(
    name = "blast_radius",
    description = "Who depends on this symbol, across files and languages (grep cannot follow imports). Use before changing or deleting a function, class or type to see every file that imports or calls it."
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
    description = "Estimate how far a change to a symbol reaches (fan-out across dependents) before a refactor, so you can scope the work without searching file by file."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PlanTool {
    /// Symbol name to analyze
    pub symbol: String,
}

#[mcp_tool(
    name = "search",
    description = "A code-only keyword search for symbols and logic (e.g. 'auth', 'database'), ranked by relevance with snippet previews. Does not search documentation: use doc_search for Markdown, ADRs and specs."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct SearchTool {
    /// Natural language query
    pub query: String,
    /// Number of context lines for snippet preview (defaults to 3)
    pub preview: Option<u32>,
}

#[mcp_tool(
    name = "fetch_symbol",
    description = "One function's or class's source without reading the whole file. Give the file path and symbol name to get just that definition."
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
    description = "Rebuild the map index after significant code changes (new files, large refactors, branch switches) so every other tool answers from current code and docs."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct InitTool {}

#[mcp_tool(
    name = "doc_search",
    description = "Find documentation sections (Markdown, ADRs, specs) about a topic by keyword, instead of grepping the docs. Returns path, anchor, heading and a snippet; read one with doc_section."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct DocSearchTool {
    /// Keywords to look for in headings, frontmatter and section text
    pub query: String,
    /// Maximum number of sections to return (defaults to 10)
    pub limit: Option<u32>,
}

#[mcp_tool(
    name = "doc_outline",
    description = "Show a Markdown document's heading tree with anchors, plus its frontmatter, so you can pick a section without reading the whole file."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct DocOutlineTool {
    /// Markdown file path
    pub path: String,
}

#[mcp_tool(
    name = "doc_section",
    description = "Read one section of a document by path and anchor (from doc_outline or doc_search), truncated at max_chars, instead of opening the whole file."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct DocSectionTool {
    /// Markdown file path
    pub path: String,
    /// Section anchor, as listed by doc_outline or doc_search
    pub anchor: String,
    /// Maximum characters of section text to return (defaults to 4000)
    pub max_chars: Option<u32>,
}

#[mcp_tool(
    name = "doc_links",
    description = "Find the documentation that mentions a code file or symbol (code to docs), or the symbols a document references (docs to code). Use before changing code to see which docs and ADRs describe it."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct DocLinksTool {
    /// File path, either a code file or a Markdown document
    pub path: String,
    /// Restrict a code file's links to one symbol
    pub symbol: Option<String>,
}

#[mcp_tool(
    name = "doc_terms",
    description = "List recurring domain vocabulary (actors, bold concepts) in the docs with the sections that use them, to learn a project's language before searching."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct DocTermsTool {
    /// Only terms from documents under this path prefix
    pub path_prefix: Option<String>,
    /// Maximum number of terms to return (defaults to 30)
    pub limit: Option<u32>,
}

#[mcp_tool(
    name = "api_surface",
    description = "List the operations (METHOD /path) and schemas declared in OpenAPI specs, without reading the YAML or JSON."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct ApiSurfaceTool {
    /// Restrict the listing to one spec file
    pub path: Option<String>,
}

// --- Server Implementation ---

pub struct McpServer {
    root: std::path::PathBuf,
    out: std::path::PathBuf,
    engine: Arc<std::sync::RwLock<Option<QueryEngine>>>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        Self::with_paths(
            std::path::PathBuf::from("."),
            std::path::PathBuf::from(".project-map"),
        )
    }

    pub fn with_paths(root: std::path::PathBuf, out: std::path::PathBuf) -> Self {
        let index_file = out.join("latest").join(".project-map.json");
        let engine = QueryEngine::load_with_retry(
            &index_file,
            crate::core::query_engine::RetryPolicy::default(),
        )
        .ok();
        Self {
            root,
            out,
            engine: Arc::new(std::sync::RwLock::new(engine)),
        }
    }

    pub fn with_engine(engine: Arc<std::sync::RwLock<Option<QueryEngine>>>) -> Self {
        Self {
            root: std::path::PathBuf::from("."),
            out: std::path::PathBuf::from(".project-map"),
            engine,
        }
    }

    pub fn engine(&self) -> Arc<std::sync::RwLock<Option<QueryEngine>>> {
        Arc::clone(&self.engine)
    }

    pub async fn run(&self) -> Result<()> {
        let _ = fmt().with_writer(std::io::stderr).try_init();

        let server_info = InitializeResult {
            protocol_version: ProtocolVersion::V2025_11_25.into(),
            capabilities: ServerCapabilities {
                tools: Some(ServerCapabilitiesTools { list_changed: None }),
                resources: Some(rust_mcp_sdk::schema::ServerCapabilitiesResources {
                    subscribe: None,
                    list_changed: None,
                }),
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
        server
            .start()
            .await
            .map_err(|e| crate::error::AppError::Generic(format!("Server error: {}", e)))?;

        Ok(())
    }

    fn clone_for_handler(&self) -> McpServerHandler {
        McpServerHandler {
            root: self.root.clone(),
            out: self.out.clone(),
            engine: Arc::clone(&self.engine),
        }
    }
}

pub struct McpServerHandler {
    root: std::path::PathBuf,
    out: std::path::PathBuf,
    engine: Arc<std::sync::RwLock<Option<QueryEngine>>>,
}

impl McpServerHandler {
    pub fn new(engine: Arc<std::sync::RwLock<Option<QueryEngine>>>) -> Self {
        Self {
            root: std::path::PathBuf::from("."),
            out: std::path::PathBuf::from(".project-map"),
            engine,
        }
    }

    pub fn with_paths(
        root: std::path::PathBuf,
        out: std::path::PathBuf,
        engine: Arc<std::sync::RwLock<Option<QueryEngine>>>,
    ) -> Self {
        Self { root, out, engine }
    }

    /// Runs `f` against the loaded engine, or reports that no index is loaded.
    fn with_engine(&self, f: impl FnOnce(&QueryEngine) -> String) -> String {
        match *self.engine.read().unwrap() {
            Some(ref engine) => f(engine),
            None => "Error: Index not loaded".to_string(),
        }
    }
}

#[async_trait]
impl ServerHandler for McpServerHandler {
    async fn handle_custom_request(
        &self,
        _request: CustomRequest,
        runtime: Arc<dyn SdkMcpServer>,
    ) -> std::result::Result<GenericResult, RpcError> {
        let server_info = runtime.server_info().to_owned();
        Ok(server_info.into())
    }

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
                DocSearchTool::tool(),
                DocOutlineTool::tool(),
                DocSectionTool::tool(),
                DocLinksTool::tool(),
                DocTermsTool::tool(),
                ApiSurfaceTool::tool(),
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
            description: Some(
                "Index status, symbol count, file count, and last indexed timestamp".to_string(),
            ),
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
            description: Some(
                "High-level project file structure and architectural pulse".to_string(),
            ),
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
        use rust_mcp_sdk::schema::{ReadResourceContent, ReadResourceResult, TextResourceContents};
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
                    "Index not initialized. Run 'project-map init' or pass --watch to build index."
                        .to_string()
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
                let latest_path = self.out.join("latest").join(".project-map.json");
                let engine_guard = self.engine.read().unwrap();
                let (tree, active_features) = if let Some(ref engine) = *engine_guard {
                    let paths = engine.get_all_file_paths();
                    let tree = render_tree(&paths, 3);
                    let active = get_active_features(&self.root);
                    (Some(tree), active)
                } else {
                    (None, Vec::new())
                };

                ToonFormatter::format_full_status(
                    engine_guard.as_ref(),
                    Some(&self.root),
                    Some(&self.out),
                    latest_path.to_str(),
                    tree.as_deref(),
                    &active_features,
                )
            }
            "query" => {
                let args: QueryTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;

                if let Some(ref engine) = *self.engine.read().unwrap() {
                    if let Some(q) = args.query {
                        let preview_lines = args.preview.map(|p| p as usize).or(Some(3));
                        let matches = engine.find_entities_with_preview(&q, preview_lines);
                        ToonFormatter::format_entity_matches(&q, &matches)
                    } else if let Some(p) = args.path {
                        let norm_p = normalize_rel_path(Path::new("."), &p);
                        let symbols = engine.get_file_outline(&norm_p);
                        ToonFormatter::format_file_context(&norm_p, &symbols)
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
                let args: BlastRadiusTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;

                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let norm_path = normalize_rel_path(Path::new("."), &args.path);
                    let results = engine.check_blast_radius(&norm_path, &args.symbol);
                    ToonFormatter::format_blast_radius(&norm_path, &args.symbol, &results)
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "plan" => {
                let args: PlanTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;

                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let impact = engine.analyze_impact(&args.symbol);
                    ToonFormatter::format_impact_analysis(&args.symbol, &impact)
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "search" => {
                let args: SearchTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;

                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let preview_lines = args.preview.map(|p| p as usize).or(Some(3));
                    let matches = engine.find_entities_with_preview(&args.query, preview_lines);
                    ToonFormatter::format_entity_matches(&args.query, &matches)
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "fetch_symbol" => {
                let args: FetchSymbolTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;

                if let Some(ref engine) = *self.engine.read().unwrap() {
                    let norm_path = normalize_rel_path(Path::new("."), &args.path);
                    if let Some(node) = engine.find_symbol_in_path(&norm_path, &args.symbol) {
                        if let Ok(content) = std::fs::read_to_string(&node.path) {
                            let bytes = content.as_bytes();
                            let content_str = if node.start_byte < bytes.len()
                                && node.end_byte <= bytes.len()
                                && node.end_byte > node.start_byte
                            {
                                String::from_utf8_lossy(&bytes[node.start_byte..node.end_byte])
                                    .to_string()
                            } else if node.line > 0 {
                                let mut extractor =
                                    crate::core::query_engine::SnippetExtractor::new();
                                extractor
                                    .extract(&node.path, node.line, 5)
                                    .map(|p| p.formatted)
                                    .unwrap_or_default()
                            } else {
                                String::new()
                            };

                            if !content_str.is_empty() {
                                ToonFormatter::format_fetch_result(
                                    &norm_path,
                                    &args.symbol,
                                    Some(&content_str),
                                )
                            } else {
                                "Error: Byte range or line out of bounds".to_string()
                            }
                        } else {
                            "Error: Could not read file".to_string()
                        }
                    } else {
                        ToonFormatter::format_fetch_result(&norm_path, &args.symbol, None)
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "doc_search" => {
                let args: DocSearchTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;
                self.with_engine(|engine| {
                    let limit = args.limit.map_or(DEFAULT_DOC_LIMIT, |l| l as usize);
                    engine.render_doc_search(&args.query, limit, DEFAULT_MAX_CHARS)
                })
            }
            "doc_outline" => {
                let args: DocOutlineTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;
                self.with_engine(|engine| engine.render_doc_outline(&args.path, DEFAULT_MAX_CHARS))
            }
            "doc_section" => {
                let args: DocSectionTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;
                self.with_engine(|engine| {
                    let max_chars = args.max_chars.map_or(DEFAULT_MAX_CHARS, |m| m as usize);
                    engine.render_doc_section(&args.path, &args.anchor, max_chars)
                })
            }
            "doc_links" => {
                let args: DocLinksTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;
                self.with_engine(|engine| {
                    engine.render_doc_links(&args.path, args.symbol.as_deref(), DEFAULT_MAX_CHARS)
                })
            }
            "doc_terms" => {
                let args: DocTermsTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;
                self.with_engine(|engine| {
                    let limit = args.limit.map_or(DEFAULT_TERMS_LIMIT, |l| l as usize);
                    engine.render_doc_terms(args.path_prefix.as_deref(), limit, DEFAULT_MAX_CHARS)
                })
            }
            "api_surface" => {
                let args: ApiSurfaceTool =
                    serde_json::from_value(arguments).map_err(|e| CallToolError(Box::new(e)))?;
                self.with_engine(|engine| {
                    engine.render_api_surface(args.path.as_deref(), DEFAULT_MAX_CHARS)
                })
            }
            "init" => match rebuild_and_publish(&self.root, &self.out) {
                Ok(new_engine) => {
                    *self.engine.write().unwrap() = Some(new_engine);
                    "Index refreshed successfully.".to_string()
                }
                Err(e) => {
                    format!("Failed to refresh index: {}", e)
                }
            },

            _ => {
                return Err(CallToolError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Unknown tool",
                ))))
            }
        };

        Ok(CallToolResult::text_content(vec![text.into()]))
    }
}
