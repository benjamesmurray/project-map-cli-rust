use tokio::io::{self, AsyncBufReadExt, BufReader, AsyncWriteExt};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::error::Result;
use crate::core::query_engine::QueryEngine;
use std::path::Path;

#[derive(Deserialize, Debug)]
struct JsonRpcRequest {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

pub struct McpServer {
    engine: Option<QueryEngine>,
}

impl McpServer {
    pub fn new() -> Self {
        let engine = QueryEngine::load(Path::new(".project-map/latest/.project-map.json")).ok();
        Self { engine }
    }

    pub async fn run(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin).lines();
        let mut stdout = io::stdout();

        while let Some(line) = reader.next_line().await? {
            let req: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let response = self.handle_request(req).await;
            let response_json = serde_json::to_string(&response)?;
            stdout.write_all(response_json.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }

        Ok(())
    }

    async fn handle_request(&mut self, req: JsonRpcRequest) -> Value {
        match req.method.as_str() {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "project-map-cli-rust",
                        "version": "0.1.0"
                    }
                }
            }),
            "notifications/initialized" => json!(null),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "result": {
                    "tools": [
                        {
                            "name": "pm_status",
                            "description": "Returns current workspace context and available commands.",
                            "inputSchema": { "type": "object", "properties": {} }
                        },
                        {
                            "name": "pm_query",
                            "description": "Search for symbols or get file context.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string" },
                                    "path": { "type": "string" }
                                }
                            }
                        },
                        {
                            "name": "pm_check_blast_radius",
                            "description": "Identifies all components and files that depend on or import a specific symbol.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "symbol": { "type": "string" }
                                },
                                "required": ["path", "symbol"]
                            }
                        },
                        {
                            "name": "pm_plan",
                            "description": "Analyze the architectural impact (fan-out) of a symbol before starting a refactor.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "symbol": { "type": "string" }
                                },
                                "required": ["symbol"]
                            }
                        },
                        {
                            "name": "pm_semantic_search",
                            "description": "Search for logic using natural language keywords (e.g., 'auth', 'database').",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string" }
                                },
                                "required": ["query"]
                            }
                        },
                        {
                            "name": "pm_fetch_symbol",
                            "description": "Extract raw source code for a specific class or function.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "symbol": { "type": "string" }
                                },
                                "required": ["path", "symbol"]
                            }
                        },
                        {
                            "name": "pm_init",
                            "description": "Refresh the map index after significant code changes to maintain discovery accuracy.",
                            "inputSchema": { "type": "object", "properties": {} }
                        }
                    ]
                }
            }),
            "tools/call" => self.handle_tool_call(req).await,
            _ => json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "error": { "code": -32601, "message": "Method not found" }
            }),
        }
    }

    async fn handle_tool_call(&mut self, req: JsonRpcRequest) -> Value {
        let params = req.params.as_ref().unwrap();
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let tool_args = params.get("arguments").cloned().unwrap_or(json!({}));

        let text = match tool_name {
            "pm_status" => {
                if self.engine.is_some() {
                    "Status: System healthy. Index is present.".to_string()
                } else {
                    "Status: Index missing. Run project-map build.".to_string()
                }
            }
            "pm_query" => {
                if let Some(ref engine) = self.engine {
                    if let Some(q) = tool_args.get("query").and_then(|v| v.as_str()) {
                        let matches = engine.find_symbols(q);
                        format!("Matches: {}", matches.len())
                    } else if let Some(p) = tool_args.get("path").and_then(|v| v.as_str()) {
                        let symbols = engine.get_file_outline(p);
                        format!("Symbols in {}: {}", p, symbols.len())
                    } else {
                        "Error: Provide query or path".to_string()
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_check_blast_radius" => {
                if let Some(ref engine) = self.engine {
                    let path = tool_args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let symbol = tool_args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    let results = engine.check_blast_radius(path, symbol);

                    if results.is_empty() {
                        "No dependent components found.".to_string()
                    } else {
                        let mut unique_files = std::collections::HashSet::new();
                        for r in &results { unique_files.insert(&r.path); }
                        format!("Blast Radius for {}:\n- Total Impacted Nodes: {}\n- Unique Files: {}\n(Top 5: {})", 
                            symbol, results.len(), unique_files.len(),
                            results.iter().take(5).map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "))
                    }
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_plan" => {
                if let Some(ref engine) = self.engine {
                    let symbol = tool_args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    let impact = engine.analyze_impact(symbol);
                    let blast = engine.check_blast_radius("", symbol);

                    let mut unique_blast = std::collections::HashSet::new();
                    for r in &blast { unique_blast.insert(&r.path); }

                    format!("Architectural Plan for {}:\n- Fan-out (Dependencies): {} nodes\n- Fan-in (Dependents): {} nodes across {} files.", 
                        symbol, impact.len(), blast.len(), unique_blast.len())
                } else {
                    "Error: Index not loaded".to_string()
                }
            }
            "pm_semantic_search" => {
                if let Some(ref engine) = self.engine {
                    let query = tool_args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                    let matches = engine.find_symbols(query);
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
                if let Some(ref engine) = self.engine {
                    let path = tool_args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let symbol = tool_args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(node) = engine.find_symbol_in_path(path, symbol) {
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
                use crate::core::orchestrator::Orchestrator;
                let mut orch = Orchestrator::new();
                if orch.build_index(Path::new(".")).is_ok() && orch.save_index_versioned(Path::new(".project-map")).is_ok() {
                    self.engine = QueryEngine::load(Path::new(".project-map/latest/.project-map.json")).ok();
                    "Index refreshed successfully.".to_string()
                } else {
                    "Failed to refresh index.".to_string()
                }
            }

            _ => "Error: Unknown tool".to_string(),
        };

        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "content": [
                    { "type": "text", "text": text }
                ]
            }
        })
    }
}
