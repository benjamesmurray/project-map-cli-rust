use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
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

use crate::core::json_out::to_json;
use crate::core::orchestrator::rebuild_and_publish;
use crate::core::query_engine::QueryEngine;
use crate::core::results::{InitResult, InitStatus};
use crate::core::tools::{index_info, run, Format, ToolCall, ToolEnv, ToolOutput};
use crate::core::utils::render_tree;
use crate::error::Result;

// --- Tool Definitions ---

// Every tool takes `format`. It is the same sentence everywhere, because the
// descriptions of deferred tools are searched by Claude Code.

#[mcp_tool(
    name = "status",
    description = "Orient in the workspace before grepping: index health, the indexed Git commit and whether it is still HEAD, the project tree, active spec features and doc counts. Start here to see what the map covers."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct StatusTool {
    /// Depth of the project tree (defaults to 3)
    pub max_depth: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

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
    /// Maximum number of matches to list (JSON defaults to 50)
    pub limit: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "blast_radius",
    description = "Who depends on this symbol, across files and languages (grep cannot follow imports). Use before changing or deleting a function, class or type to see every file that imports it, with direct and transitive counts. Counts dependents (importers, stream and config links), not callers: the index has no call edges."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct BlastRadiusTool {
    /// File path where the symbol is defined
    pub path: String,
    /// Symbol name to check
    pub symbol: String,
    /// Maximum number of dependents to list; the counts cover all (JSON defaults to 200)
    pub limit: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "plan",
    description = "Estimate how far a change to a symbol reaches (fan-out across dependents) before a refactor, so you can scope the work without searching file by file."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct PlanTool {
    /// Symbol name to analyze
    pub symbol: String,
    /// Maximum number of nodes to list (JSON defaults to 200)
    pub limit: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
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
    /// Maximum number of matches to list (JSON defaults to 50)
    pub limit: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
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
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "init",
    description = "Rebuild the map index after significant code changes (new files, large refactors, branch switches) so every other tool answers from current code and docs."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct InitTool {
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

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
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "doc_outline",
    description = "Show a Markdown document's heading tree with anchors, plus its frontmatter, so you can pick a section without reading the whole file."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct DocOutlineTool {
    /// Markdown file path
    pub path: String,
    /// Keep headings down to this depth (1 = top level only); omit for all
    pub max_depth: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
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
    /// Maximum characters of section text to return (defaults to 4000; JSON returns the full text unless given)
    pub max_chars: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
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
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
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
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "api_surface",
    description = "List the operations (METHOD /path) and schemas declared in OpenAPI specs, without reading the YAML or JSON."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct ApiSurfaceTool {
    /// Restrict the listing to one spec file
    pub path: Option<String>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "modules",
    description = "Which areas of the codebase are largest, by files, symbols and doc sections: recursive counts per directory, instead of listing the tree and counting by hand."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct ModulesTool {
    /// Only directories under this path prefix (defaults to the whole repository)
    pub path_prefix: Option<String>,
    /// Directory levels below the prefix to list (defaults to 2)
    pub max_depth: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

#[mcp_tool(
    name = "imports",
    description = "Which files use an external package, with the import line (grep cannot tell an import from a mention). Give a package name such as stripe or sentry_sdk, or a prefix such as @sentry/*."
)]
#[derive(JsonSchema, Deserialize, Serialize)]
pub struct ImportsTool {
    /// Package name, exact (stripe, serde_json) or by prefix with a trailing /* or .* (@sentry/*)
    pub package: String,
    /// Only files under this path prefix
    pub path_prefix: Option<String>,
    /// Maximum number of import lines to list; the counts cover all (defaults to 50)
    pub limit: Option<u32>,
    /// Output format: "toon" (default, compact text) or "json" (for programs, costs more tokens)
    pub format: Option<String>,
}

/// What the server tells the model at session start, even when the client defers the
/// tools (Claude Code shows only tool names and these instructions until a tool is
/// searched for). Claude Code cuts them at 4,096 characters.
pub const SERVER_INSTRUCTIONS: &str = "\
Project Map indexes this repository: code symbols and the imports between files across languages, Markdown documentation (ADRs, specs, READMEs) and OpenAPI files. Use its tools instead of grep or reading whole files for structural questions.

- status: start here; index health, indexed Git commit and whether it is still HEAD, project tree.
- query, search, fetch_symbol: where a symbol is defined, a file's outline, one definition's source.
- blast_radius, plan: who depends on a symbol (importers, not callers) and how far a change reaches.
- imports: which files use an external package, with the import line.
- modules: which areas are largest, by files, symbols and doc sections.
- doc_search, doc_outline, doc_section, doc_links, doc_terms, api_surface: documentation and API specs.
- init: rebuild the index when status says it is missing or stale.

Every tool takes format: \"toon\" (default, compact text) or \"json\" (a stable envelope for programs; costs more tokens). Lists come in a fixed order.";

/// The initialize result: server identity, capabilities and the instructions the model
/// sees at session start.
pub fn server_details() -> InitializeResult {
    InitializeResult {
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
        instructions: Some(SERVER_INSTRUCTIONS.to_string()),
        meta: None,
    }
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
        // Colour only for a person at a terminal: a job's log collector gets plain text.
        let _ = fmt()
            .with_writer(std::io::stderr)
            .with_ansi(std::io::stderr().is_terminal())
            .try_init();

        let server_info = server_details();

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
                ModulesTool::tool(),
                ImportsTool::tool(),
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
        let env = ToolEnv {
            root: &self.root,
            out: &self.out,
        };
        let engine_guard = self.engine.read().unwrap();
        let engine = engine_guard.as_ref();
        let usize_of = |v: Option<u32>| v.map(|n| n as usize);

        macro_rules! args {
            ($t:ty) => {
                serde_json::from_value::<$t>(arguments).map_err(|e| CallToolError(Box::new(e)))?
            };
        }
        // Parses `format`, runs the call, and shapes the MCP result.
        macro_rules! respond {
            ($format:expr, $call:expr) => {
                match Format::parse($format.as_deref()) {
                    Ok(format) => run(engine, env, &$call, format),
                    Err(e) => ToolOutput::failure(&e),
                }
            };
        }

        let output = match params.name.as_str() {
            "status" => {
                let a = args!(StatusTool);
                respond!(
                    a.format,
                    ToolCall::Status {
                        max_depth: usize_of(a.max_depth)
                    }
                )
            }
            "query" => {
                let a = args!(QueryTool);
                respond!(
                    a.format,
                    ToolCall::Query {
                        query: a.query.as_deref(),
                        path: a.path.as_deref(),
                        filename: a.filename.as_deref(),
                        preview: a.preview.map(|p| p as usize).or(Some(3)),
                        limit: usize_of(a.limit),
                    }
                )
            }
            "blast_radius" => {
                let a = args!(BlastRadiusTool);
                respond!(
                    a.format,
                    ToolCall::BlastRadius {
                        path: &a.path,
                        symbol: &a.symbol,
                        limit: usize_of(a.limit),
                    }
                )
            }
            "plan" => {
                let a = args!(PlanTool);
                respond!(
                    a.format,
                    ToolCall::Plan {
                        symbol: &a.symbol,
                        limit: usize_of(a.limit),
                    }
                )
            }
            "search" => {
                let a = args!(SearchTool);
                respond!(
                    a.format,
                    ToolCall::Search {
                        query: &a.query,
                        preview: a.preview.map(|p| p as usize).or(Some(3)),
                        limit: usize_of(a.limit),
                    }
                )
            }
            "fetch_symbol" => {
                let a = args!(FetchSymbolTool);
                respond!(
                    a.format,
                    ToolCall::FetchSymbol {
                        path: &a.path,
                        symbol: &a.symbol,
                    }
                )
            }
            "doc_search" => {
                let a = args!(DocSearchTool);
                respond!(
                    a.format,
                    ToolCall::DocSearch {
                        query: &a.query,
                        limit: usize_of(a.limit),
                    }
                )
            }
            "doc_outline" => {
                let a = args!(DocOutlineTool);
                respond!(
                    a.format,
                    ToolCall::DocOutline {
                        path: &a.path,
                        max_depth: usize_of(a.max_depth),
                    }
                )
            }
            "doc_section" => {
                let a = args!(DocSectionTool);
                respond!(
                    a.format,
                    ToolCall::DocSection {
                        path: &a.path,
                        anchor: &a.anchor,
                        max_chars: usize_of(a.max_chars),
                    }
                )
            }
            "doc_links" => {
                let a = args!(DocLinksTool);
                respond!(
                    a.format,
                    ToolCall::DocLinks {
                        path: &a.path,
                        symbol: a.symbol.as_deref(),
                    }
                )
            }
            "doc_terms" => {
                let a = args!(DocTermsTool);
                respond!(
                    a.format,
                    ToolCall::DocTerms {
                        path_prefix: a.path_prefix.as_deref(),
                        limit: usize_of(a.limit),
                    }
                )
            }
            "api_surface" => {
                let a = args!(ApiSurfaceTool);
                respond!(
                    a.format,
                    ToolCall::ApiSurface {
                        path: a.path.as_deref()
                    }
                )
            }
            "modules" => {
                let a = args!(ModulesTool);
                respond!(
                    a.format,
                    ToolCall::Modules {
                        path_prefix: a.path_prefix.as_deref(),
                        max_depth: usize_of(a.max_depth),
                    }
                )
            }
            "imports" => {
                let a = args!(ImportsTool);
                respond!(
                    a.format,
                    ToolCall::Imports {
                        package: &a.package,
                        path_prefix: a.path_prefix.as_deref(),
                        limit: usize_of(a.limit),
                    }
                )
            }
            "init" => {
                let a = args!(InitTool);
                drop(engine_guard);
                self.init(a.format.as_deref())
            }
            _ => {
                return Err(CallToolError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Unknown tool",
                ))))
            }
        };

        let mut result = CallToolResult::text_content(vec![output.text.clone().into()]);
        result.structured_content = output.structured();
        if output.is_error {
            result.is_error = Some(true);
        }
        Ok(result)
    }
}

impl McpServerHandler {
    /// Rebuilds the index and swaps the engine. The summary carries the new commit and
    /// dirty flag, so a job needs no second `status` call.
    fn init(&self, format: Option<&str>) -> ToolOutput {
        let format = match Format::parse(format) {
            Ok(f) => f,
            Err(e) => return ToolOutput::failure(&e),
        };
        let (result, is_error) = match rebuild_and_publish(&self.root, &self.out) {
            Ok(new_engine) => {
                let meta = new_engine.meta();
                let result = InitResult {
                    status: InitStatus::Refreshed,
                    message: "Index refreshed successfully.".to_string(),
                    commit: meta.and_then(|m| m.git_commit.clone()),
                    dirty: meta.and_then(|m| m.git_dirty),
                };
                *self.engine.write().unwrap() = Some(new_engine);
                (result, false)
            }
            Err(e) => (
                InitResult {
                    status: InitStatus::Failed,
                    message: format!("Failed to refresh index: {}", e),
                    commit: None,
                    dirty: None,
                },
                true,
            ),
        };
        match format {
            Format::Toon => {
                let mut text = result.message.clone();
                if let (Some(commit), false) = (&result.commit, is_error) {
                    let dirty = match result.dirty {
                        Some(true) => " (dirty)",
                        Some(false) => " (clean)",
                        None => "",
                    };
                    text.push_str(&format!(" Commit: {}{}", commit, dirty));
                }
                ToolOutput {
                    text,
                    json: None,
                    is_error,
                }
            }
            Format::Json => {
                let engine = self.engine.read().unwrap();
                let value = to_json("init", &Ok(result), &index_info(engine.as_ref()));
                ToolOutput {
                    text: value.to_string(),
                    json: Some(value),
                    is_error,
                }
            }
        }
    }
}
