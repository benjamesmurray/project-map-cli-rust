//! One entry point for every query tool, shared by the MCP server and the CLI: a
//! `ToolCall` and a `Format` in, the text to print (TOON or the JSON envelope) and the
//! JSON object out. TOON output is the 0.3.0 text; JSON is the contract envelope.

use crate::core::json_out::{error_json, to_json, ToPayload};
use crate::core::query_engine::QueryEngine;
use crate::core::results::{IndexInfo, QueryError, StatusResult};
use crate::core::toon::{ToonFormatter, DEFAULT_MAX_CHARS};
use crate::core::utils::{get_active_features, normalize_rel_path, render_tree};
use serde_json::{Map, Value};
use std::path::Path;

/// Item limits of the JSON format (Specification, item 1). TOON has none by default.
pub const JSON_LIMIT_SEARCH: usize = 50;
pub const JSON_LIMIT_WALK: usize = 200;
pub const DEFAULT_DOC_LIMIT: usize = 10;
pub const DEFAULT_TERMS_LIMIT: usize = 30;
pub const DEFAULT_TREE_DEPTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    #[default]
    Toon,
    Json,
}

impl Format {
    /// `toon` (also when absent) or `json`; anything else is `invalid_argument`.
    pub fn parse(value: Option<&str>) -> Result<Self, QueryError> {
        match value {
            None | Some("toon") => Ok(Format::Toon),
            Some("json") => Ok(Format::Json),
            Some(other) => Err(QueryError::invalid_argument(format!(
                "Error: Invalid format '{other}': use 'toon' or 'json'"
            ))),
        }
    }
}

/// The outcome of one tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    /// TOON text, or the JSON document as compact text.
    pub text: String,
    /// The envelope, for `json` only.
    pub json: Option<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    fn toon(text: String, is_error: bool) -> Self {
        Self {
            text,
            json: None,
            is_error,
        }
    }

    fn json(value: Value, is_error: bool) -> Self {
        Self {
            text: value.to_string(),
            json: Some(value),
            is_error,
        }
    }

    /// The envelope as the object MCP's `structured_content` carries.
    pub fn structured(&self) -> Option<Map<String, Value>> {
        match &self.json {
            Some(Value::Object(map)) => Some(map.clone()),
            _ => None,
        }
    }

    /// A failure before any tool ran (for example an invalid `format`).
    pub fn failure(error: &QueryError) -> Self {
        Self::toon(error.message.clone(), true)
    }
}

/// A query tool and its arguments. `init` is not here: it builds an index.
#[derive(Debug, Clone)]
pub enum ToolCall<'a> {
    Status {
        max_depth: Option<usize>,
    },
    Query {
        query: Option<&'a str>,
        path: Option<&'a str>,
        filename: Option<&'a str>,
        preview: Option<usize>,
        limit: Option<usize>,
    },
    Search {
        query: &'a str,
        preview: Option<usize>,
        limit: Option<usize>,
    },
    FetchSymbol {
        path: &'a str,
        symbol: &'a str,
    },
    BlastRadius {
        path: &'a str,
        symbol: &'a str,
        limit: Option<usize>,
    },
    Plan {
        symbol: &'a str,
        limit: Option<usize>,
    },
    DocSearch {
        query: &'a str,
        limit: Option<usize>,
    },
    DocOutline {
        path: &'a str,
        max_depth: Option<usize>,
    },
    DocSection {
        path: &'a str,
        anchor: &'a str,
        max_chars: Option<usize>,
    },
    DocLinks {
        path: &'a str,
        symbol: Option<&'a str>,
    },
    DocTerms {
        path_prefix: Option<&'a str>,
        limit: Option<usize>,
    },
    ApiSurface {
        path: Option<&'a str>,
    },
    Modules {
        path_prefix: Option<&'a str>,
        max_depth: Option<usize>,
    },
    Imports {
        package: &'a str,
        path_prefix: Option<&'a str>,
        limit: Option<usize>,
    },
}

impl ToolCall<'_> {
    pub fn name(&self) -> &'static str {
        match self {
            ToolCall::Status { .. } => "status",
            ToolCall::Query { .. } => "query",
            ToolCall::Search { .. } => "search",
            ToolCall::FetchSymbol { .. } => "fetch_symbol",
            ToolCall::BlastRadius { .. } => "blast_radius",
            ToolCall::Plan { .. } => "plan",
            ToolCall::DocSearch { .. } => "doc_search",
            ToolCall::DocOutline { .. } => "doc_outline",
            ToolCall::DocSection { .. } => "doc_section",
            ToolCall::DocLinks { .. } => "doc_links",
            ToolCall::DocTerms { .. } => "doc_terms",
            ToolCall::ApiSurface { .. } => "api_surface",
            ToolCall::Modules { .. } => "modules",
            ToolCall::Imports { .. } => "imports",
        }
    }
}

/// Where the server or CLI works: the project root (for the active features) and the
/// index output directory (for `status`).
#[derive(Debug, Clone, Copy)]
pub struct ToolEnv<'a> {
    pub root: &'a Path,
    pub out: &'a Path,
}

/// The envelope's index object: `missing` without an engine.
pub fn index_info(engine: Option<&QueryEngine>) -> IndexInfo {
    engine.map_or_else(IndexInfo::missing, QueryEngine::index_info)
}

/// Appends a note to TOON text when a `limit` cut the listing.
fn limit_note(total: usize, limit: Option<usize>) -> String {
    match limit {
        Some(l) if total > l => format!("\n[limit {l}: {total} matched]"),
        _ => String::new(),
    }
}

fn json_result<T: ToPayload>(
    tool: &str,
    result: Result<T, QueryError>,
    engine: &QueryEngine,
) -> ToolOutput {
    let is_error = result.is_err();
    ToolOutput::json(to_json(tool, &result, &index_info(Some(engine))), is_error)
}

/// A TOON result or the error's message.
fn toon_result<T>(result: Result<T, QueryError>, render: impl FnOnce(T) -> String) -> ToolOutput {
    match result {
        Ok(r) => ToolOutput::toon(render(r), false),
        Err(e) => ToolOutput::toon(e.message, true),
    }
}

/// Runs one tool. Without an engine every tool but `status` answers `index_missing`.
pub fn run(
    engine: Option<&QueryEngine>,
    env: ToolEnv<'_>,
    call: &ToolCall<'_>,
    format: Format,
) -> ToolOutput {
    if let ToolCall::Status { max_depth } = call {
        return run_status(engine, env, *max_depth, format);
    }
    let Some(engine) = engine else {
        let error = QueryError::index_missing();
        return match format {
            Format::Toon => ToolOutput::failure(&error),
            Format::Json => {
                ToolOutput::json(error_json(call.name(), &error, &IndexInfo::missing()), true)
            }
        };
    };
    match format {
        Format::Toon => run_toon(engine, call),
        Format::Json => run_json(engine, call),
    }
}

fn run_status(
    engine: Option<&QueryEngine>,
    env: ToolEnv<'_>,
    max_depth: Option<usize>,
    format: Format,
) -> ToolOutput {
    let depth = max_depth.unwrap_or(DEFAULT_TREE_DEPTH);
    match format {
        Format::Toon => {
            let latest = env.out.join("latest").join(".project-map.json");
            let (tree, active_features) = match engine {
                Some(e) => {
                    let paths = e.get_all_file_paths();
                    let mut tree = render_tree(&paths, depth);
                    if max_depth.is_some() {
                        let (_, cut) =
                            crate::core::utils::PathTree::from_paths(&paths).to_result(".", depth);
                        if cut {
                            tree.push_str(&format!("\n[tree cut at depth {depth}]\n"));
                        }
                    }
                    (Some(tree), get_active_features(env.root))
                }
                None => (None, Vec::new()),
            };
            ToolOutput::toon(
                ToonFormatter::format_full_status(
                    engine,
                    Some(env.root),
                    Some(env.out),
                    latest.to_str(),
                    tree.as_deref(),
                    &active_features,
                ),
                false,
            )
        }
        Format::Json => {
            let (status, info) = match engine {
                Some(e) => (
                    e.status_result(depth, get_active_features(env.root)),
                    e.index_info(),
                ),
                None => (StatusResult::missing(Vec::new()), IndexInfo::missing()),
            };
            ToolOutput::json(to_json("status", &Ok(status), &info), false)
        }
    }
}

fn run_toon(engine: &QueryEngine, call: &ToolCall<'_>) -> ToolOutput {
    match call {
        ToolCall::Status { .. } => unreachable!("status is handled before the engine check"),
        ToolCall::Query {
            query,
            path,
            filename,
            preview,
            limit,
        } => {
            if let Some(q) = query {
                return toon_entities(engine, q, *preview, *limit);
            }
            if let Some(p) = path {
                let norm = normalize_rel_path(Path::new("."), p);
                let symbols = engine.get_file_outline(&norm);
                return ToolOutput::toon(
                    ToonFormatter::format_file_context(&norm, &symbols),
                    false,
                );
            }
            if let Some(f) = filename {
                let mut matches = engine.find_files(f);
                let total = matches.len();
                if let Some(l) = limit {
                    matches.truncate(*l);
                }
                let text = ToonFormatter::format_file_matches(f, &matches);
                return ToolOutput::toon(text + &limit_note(total, *limit), false);
            }
            ToolOutput::failure(&QueryError::invalid_argument(
                "Error: Provide query, path, or filename",
            ))
        }
        ToolCall::Search {
            query,
            preview,
            limit,
        } => toon_entities(engine, query, *preview, *limit),
        ToolCall::FetchSymbol { path, symbol } => {
            let norm = normalize_rel_path(Path::new("."), path);
            match engine.fetch_result(path, symbol) {
                Ok(f) => ToolOutput::toon(
                    ToonFormatter::format_fetch_result(&norm, symbol, Some(&f.source)),
                    false,
                ),
                Err(e) if e.code == crate::core::results::ErrorCode::NotFound => ToolOutput::toon(
                    ToonFormatter::format_fetch_result(&norm, symbol, None),
                    false,
                ),
                Err(e) => ToolOutput::failure(&e),
            }
        }
        ToolCall::BlastRadius {
            path,
            symbol,
            limit,
        } => {
            let norm = normalize_rel_path(Path::new("."), path);
            let (radius, mut nodes) = engine.blast_with_nodes(&norm, symbol);
            if let Some(l) = limit {
                nodes.truncate(*l);
            }
            ToolOutput::toon(
                ToonFormatter::format_blast_radius(&norm, symbol, &nodes, &radius)
                    + &limit_note(radius.total, *limit),
                false,
            )
        }
        ToolCall::Plan { symbol, limit } => {
            let mut impact = engine.analyze_impact(symbol);
            let total = impact.len();
            if let Some(l) = limit {
                impact.truncate(*l);
            }
            ToolOutput::toon(
                ToonFormatter::format_impact_analysis(symbol, &impact) + &limit_note(total, *limit),
                false,
            )
        }
        ToolCall::DocSearch { query, limit } => ToolOutput::toon(
            engine.render_doc_search(query, limit.unwrap_or(DEFAULT_DOC_LIMIT), DEFAULT_MAX_CHARS),
            false,
        ),
        ToolCall::DocOutline { path, max_depth } => {
            let view = engine
                .doc_outline(path)
                .map_err(QueryError::from_doc_message);
            toon_result(view, |view| {
                let (view, cut) = match max_depth {
                    Some(d) => view.cut_to_depth(*d),
                    None => (view, false),
                };
                let mut text = ToonFormatter::format_doc_outline(&view, DEFAULT_MAX_CHARS);
                if cut {
                    text.push_str(&format!(
                        "\n[outline cut at depth {}]",
                        max_depth.unwrap_or_default()
                    ));
                }
                text
            })
        }
        ToolCall::DocSection {
            path,
            anchor,
            max_chars,
        } => ToolOutput::toon(
            engine.render_doc_section(path, anchor, max_chars.unwrap_or(DEFAULT_MAX_CHARS)),
            false,
        ),
        ToolCall::DocLinks { path, symbol } => ToolOutput::toon(
            engine.render_doc_links(path, *symbol, DEFAULT_MAX_CHARS),
            false,
        ),
        ToolCall::DocTerms { path_prefix, limit } => ToolOutput::toon(
            engine.render_doc_terms(
                *path_prefix,
                limit.unwrap_or(DEFAULT_TERMS_LIMIT),
                DEFAULT_MAX_CHARS,
            ),
            false,
        ),
        ToolCall::ApiSurface { path } => {
            ToolOutput::toon(engine.render_api_surface(*path, DEFAULT_MAX_CHARS), false)
        }
        ToolCall::Modules {
            path_prefix,
            max_depth,
        } => toon_result(
            engine.modules_result(*path_prefix, max_depth.unwrap_or(2)),
            |r| ToonFormatter::format_modules(&r, DEFAULT_MAX_CHARS),
        ),
        ToolCall::Imports {
            package,
            path_prefix,
            limit,
        } => toon_result(engine.imports_result(package, *path_prefix, *limit), |r| {
            ToonFormatter::format_imports(&r, DEFAULT_MAX_CHARS)
        }),
    }
}

fn toon_entities(
    engine: &QueryEngine,
    query: &str,
    preview: Option<usize>,
    limit: Option<usize>,
) -> ToolOutput {
    let mut matches = engine.find_entities_with_preview(query, preview);
    let total = matches.len();
    if let Some(l) = limit {
        matches.truncate(l);
    }
    let text = ToonFormatter::format_entity_matches(query, &matches);
    ToolOutput::toon(text + &limit_note(total, limit), false)
}

fn run_json(engine: &QueryEngine, call: &ToolCall<'_>) -> ToolOutput {
    let tool = call.name();
    match call {
        ToolCall::Status { .. } => unreachable!("status is handled before the engine check"),
        ToolCall::Query {
            query,
            path,
            filename,
            preview,
            limit,
        } => {
            let limit = Some(limit.unwrap_or(JSON_LIMIT_SEARCH));
            if let Some(q) = query {
                json_result(tool, Ok(engine.entity_list(q, *preview, limit)), engine)
            } else if let Some(p) = path {
                json_result(tool, engine.outline_result(p), engine)
            } else if let Some(f) = filename {
                json_result(tool, Ok(engine.files_result(f, limit)), engine)
            } else {
                json_result::<crate::core::results::EntityList>(
                    tool,
                    Err(QueryError::invalid_argument(
                        "Error: Provide query, path, or filename",
                    )),
                    engine,
                )
            }
        }
        ToolCall::Search {
            query,
            preview,
            limit,
        } => json_result(
            tool,
            Ok(engine.entity_list(query, *preview, Some(limit.unwrap_or(JSON_LIMIT_SEARCH)))),
            engine,
        ),
        ToolCall::FetchSymbol { path, symbol } => {
            json_result(tool, engine.fetch_result(path, symbol), engine)
        }
        ToolCall::BlastRadius {
            path,
            symbol,
            limit,
        } => json_result(
            tool,
            Ok(engine.blast_result(path, symbol, Some(limit.unwrap_or(JSON_LIMIT_WALK)))),
            engine,
        ),
        ToolCall::Plan { symbol, limit } => json_result(
            tool,
            Ok(engine.plan_result(symbol, Some(limit.unwrap_or(JSON_LIMIT_WALK)))),
            engine,
        ),
        ToolCall::DocSearch { query, limit } => json_result(
            tool,
            engine.doc_search_result(query, limit.unwrap_or(DEFAULT_DOC_LIMIT)),
            engine,
        ),
        ToolCall::DocOutline { path, max_depth } => {
            json_result(tool, engine.doc_outline_result(path, *max_depth), engine)
        }
        ToolCall::DocSection {
            path,
            anchor,
            max_chars,
        } => json_result(
            tool,
            engine.doc_section_result(path, anchor, *max_chars),
            engine,
        ),
        ToolCall::DocLinks { path, symbol } => {
            json_result(tool, engine.doc_links_result(path, *symbol), engine)
        }
        ToolCall::DocTerms { path_prefix, limit } => json_result(
            tool,
            engine.doc_terms_result(*path_prefix, limit.unwrap_or(DEFAULT_TERMS_LIMIT)),
            engine,
        ),
        ToolCall::ApiSurface { path } => {
            json_result(tool, engine.api_surface_result(*path), engine)
        }
        ToolCall::Modules {
            path_prefix,
            max_depth,
        } => json_result(
            tool,
            engine.modules_result(*path_prefix, max_depth.unwrap_or(2)),
            engine,
        ),
        ToolCall::Imports {
            package,
            path_prefix,
            limit,
        } => json_result(
            tool,
            engine.imports_result(package, *path_prefix, *limit),
            engine,
        ),
    }
}
