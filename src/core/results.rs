//! Typed results of the tools, with the stable field names of the JSON contract
//! (Specification, Data Models). A query returns one of these (or a `QueryError`);
//! the TOON formatter and the JSON renderer both consume it, so the two formats see
//! the same data in the same order.
//!
//! A field that does not apply is omitted, never `null`. The exceptions are the
//! envelope's `index` fields and the Git fields of `status`, which are `null` when
//! unknown.

use crate::core::graph::{DocMeta, IndexMeta, NodeData, NodeType, STALE_INDEX_MESSAGE};
use serde::{Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;

// --- Errors ---

/// The error codes of the JSON contract.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    IndexMissing,
    IndexStale,
    NotFound,
    InvalidArgument,
    IoError,
}

/// A user-facing problem. TOON prints `message` exactly as 0.3.0 printed its error
/// strings; JSON adds the `code`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueryError {
    pub code: ErrorCode,
    pub message: String,
}

impl QueryError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn index_missing() -> Self {
        Self::new(ErrorCode::IndexMissing, "Error: Index not loaded")
    }

    pub fn index_stale() -> Self {
        Self::new(ErrorCode::IndexStale, STALE_INDEX_MESSAGE)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn io_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::IoError, message)
    }

    /// Classifies a doc-query message: the stale-index and stale-range messages are
    /// `index_stale`; every other problem (unknown path, anchor, symbol, no API
    /// files) is `not_found`.
    pub fn from_doc_message(message: String) -> Self {
        let code = if message == STALE_INDEX_MESSAGE || message.starts_with("Stale range:") {
            ErrorCode::IndexStale
        } else {
            ErrorCode::NotFound
        };
        Self { code, message }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for QueryError {}

// --- Envelope: index information ---

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    Ok,
    Stale,
    Missing,
}

/// The envelope's `index` object. With `missing`, the other fields are `null`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IndexInfo {
    pub state: IndexState,
    pub schema_version: Option<u32>,
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub stale: Option<bool>,
}

impl IndexInfo {
    pub fn missing() -> Self {
        Self {
            state: IndexState::Missing,
            schema_version: None,
            commit: None,
            dirty: None,
            stale: None,
        }
    }

    /// `meta` is `None` for a legacy index without a meta block; `stale` is the
    /// engine's verdict (schema older than this build).
    pub fn loaded(meta: Option<&IndexMeta>, stale: bool) -> Self {
        Self {
            state: if stale {
                IndexState::Stale
            } else {
                IndexState::Ok
            },
            schema_version: Some(meta.map_or(0, |m| m.schema_version)),
            commit: meta.and_then(|m| m.git_commit.clone()),
            dirty: meta.and_then(|m| m.git_dirty),
            stale: Some(stale),
        }
    }
}

// --- Shared helpers ---

fn serialize_node_type<S: Serializer>(t: &NodeType, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(t.as_str())
}

/// Names a symbol by the symbols that enclose it in the same file: a symbol whose
/// byte range lies inside another symbol's range is `Outer.inner`, nested to any
/// depth. A Rust `impl Trait for Type` block is already named after `Type` in the
/// index, so its methods come out as `Type.method`.
///
/// Qualified names are **not unique**: two `impl From<..> for E` blocks both give
/// `E.from`. A citation needs the path and line as well.
#[derive(Debug, Default)]
pub struct QualifiedNamer {
    /// Per path: (start_byte, end_byte, name) of every code symbol.
    /// Lookup-only after construction; each list is sorted.
    by_path: HashMap<String, Vec<(usize, usize, String)>>,
}

impl QualifiedNamer {
    pub fn new<'a>(nodes: impl Iterator<Item = &'a NodeData>) -> Self {
        let mut by_path: HashMap<String, Vec<(usize, usize, String)>> = HashMap::new();
        for n in nodes.filter(|n| n.node_type == NodeType::Symbol && n.end_byte > n.start_byte) {
            by_path.entry(n.path.clone()).or_default().push((
                n.start_byte,
                n.end_byte,
                n.name.clone(),
            ));
        }
        for symbols in by_path.values_mut() {
            // Outermost first: earlier start, then the larger range.
            symbols.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        }
        Self { by_path }
    }

    pub fn qualify(&self, node: &NodeData) -> String {
        let Some(symbols) = self.by_path.get(&node.path) else {
            return node.name.clone();
        };
        let mut parts: Vec<&str> = Vec::new();
        for (start, end, name) in symbols {
            if *start > node.start_byte {
                break;
            }
            let strictly_encloses =
                *end >= node.end_byte && (*start, *end) != (node.start_byte, node.end_byte);
            if strictly_encloses {
                parts.push(name);
            }
        }
        parts.push(&node.name);
        parts.join(".")
    }
}

/// A node's 1-based line, or `None` for a file node, which the index stores at line 0.
pub fn line_of(line: usize) -> Option<usize> {
    (line > 0).then_some(line)
}

/// 1-based last line of a node whose first line is `line`, read from the file text
/// by the node's byte range. A trailing line break belongs to the last line. Falls
/// back to `line` when the range does not fit `source` (the file changed).
pub fn end_line_of(source: &str, line: usize, start_byte: usize, end_byte: usize) -> usize {
    match source.get(start_byte..end_byte) {
        Some(range) if end_byte > start_byte => {
            line + range.trim_end_matches(['\n', '\r']).matches('\n').count()
        }
        _ => line,
    }
}

// --- Code tools ---

/// A symbol found by `search` or `query`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SymbolItem {
    /// Qualified (`Outer.inner`) when inside another symbol; not unique.
    pub symbol: String,
    pub kind: String,
    #[serde(serialize_with = "serialize_node_type")]
    pub node_type: NodeType,
    pub path: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// `search` and the query form of `query`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EntityList {
    pub query: String,
    /// Matches before any limit was applied.
    pub total: usize,
    pub truncated: bool,
    pub items: Vec<SymbolItem>,
}

/// One entry of a file outline. An import entry names the imported file or symbol
/// and carries the `Import` role and that node's path.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OutlineItem {
    pub symbol: String,
    pub kind: String,
    /// Omitted for a file node, which has no line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The path form of `query`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileOutlineResult {
    pub path: String,
    pub items: Vec<OutlineItem>,
}

/// The filename form of `query`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileList {
    pub query: String,
    pub total: usize,
    pub truncated: bool,
    pub paths: Vec<String>,
}

/// `fetch_symbol`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FetchResult {
    pub path: String,
    pub symbol: String,
    pub line: usize,
    pub end_line: usize,
    pub source: String,
}

/// Which node a blast radius started from.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlastMatch {
    Symbol,
    File,
    NotFound,
}

/// A dependent in a blast radius. `depth` is 1 for a direct dependent and 2 or more
/// for a transitive one.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlastItem {
    pub symbol: String,
    pub kind: String,
    #[serde(serialize_with = "serialize_node_type")]
    pub node_type: NodeType,
    pub path: String,
    /// Omitted for a file node, which has no line (and so no `end_line`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    pub depth: usize,
}

/// `blast_radius`. `direct` counts dependents (importers, stream and config links),
/// not callers: the graph has no call edges. Every count covers all dependents, not
/// only the listed items.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlastRadiusResult {
    pub target_path: String,
    /// Omitted when `matched` is `file` or `not_found`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_line: Option<usize>,
    pub matched: BlastMatch,
    /// Set when the target is nested in another symbol (a method): the outermost
    /// enclosing symbol the counts were also taken from, because no import names a
    /// method. The counts then include importers who never use this method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Dependents at depth 1.
    pub direct: usize,
    /// Dependents at depth 2 or more.
    pub transitive: usize,
    pub total: usize,
    /// Distinct paths of dependents, excluding the target's own file.
    pub files_affected: usize,
    /// Distinct languages of the affected files, sorted.
    pub languages: Vec<String>,
    pub truncated: bool,
    pub items: Vec<BlastItem>,
}

/// A node `plan` reaches from its target.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlanItem {
    pub symbol: String,
    pub kind: String,
    #[serde(serialize_with = "serialize_node_type")]
    pub node_type: NodeType,
    pub path: String,
    /// Omitted for a file node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Edges from the target (breadth-first, shortest).
    pub depth: usize,
}

/// `plan`. The target is what the requested name resolved to; it is absent when
/// nothing matched.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlanResult {
    pub requested: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_line: Option<usize>,
    pub total: usize,
    pub truncated: bool,
    pub items: Vec<PlanItem>,
}

// --- status ---

/// A directory of the module tree, to the requested depth.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

/// `status`. `commit`, `dirty` and `branch` come from the index (schema 4);
/// `head_now`, `dirty_now` and `at_head` are read at query time. Git fields are
/// `null` when unknown. A missing index is a `StatusResult` too (`missing`), not an error.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StatusResult {
    pub tool_version: String,
    pub schema_version: u32,
    pub root: String,
    pub built_at: String,
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub branch: Option<String>,
    pub head_now: Option<String>,
    pub dirty_now: Option<bool>,
    pub at_head: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_error: Option<String>,
    pub stale: bool,
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
    pub doc_files: usize,
    pub doc_sections: usize,
    pub doc_references: usize,
    pub api_operations: usize,
    pub tree: TreeNode,
    pub active_features: Vec<String>,
    /// Whether the tree was cut at `max_depth`. Reported in the envelope's `truncated`.
    #[serde(skip)]
    pub truncated: bool,
    /// Whether an index was loaded. Reported in the envelope's `index.state`.
    #[serde(skip)]
    pub missing: bool,
    /// What to do about a missing index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

pub const MISSING_INDEX_NEXT_STEP: &str =
    "Run `project-map refresh` (or call the `init` tool) to build the index.";

impl StatusResult {
    /// The result when no index is loaded: zero counts, an empty tree, the Git fields
    /// `null`, and the instruction to build one.
    pub fn missing(active_features: Vec<String>) -> Self {
        Self {
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: 0,
            root: String::new(),
            built_at: String::new(),
            commit: None,
            dirty: None,
            branch: None,
            head_now: None,
            dirty_now: None,
            at_head: None,
            git_error: None,
            stale: false,
            files: 0,
            symbols: 0,
            edges: 0,
            doc_files: 0,
            doc_sections: 0,
            doc_references: 0,
            api_operations: 0,
            tree: TreeNode {
                name: ".".to_string(),
                children: Vec::new(),
            },
            active_features,
            truncated: false,
            missing: true,
            next_step: Some(MISSING_INDEX_NEXT_STEP.to_string()),
        }
    }
}

// --- Documentation tools ---

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocSearchItem {
    pub path: String,
    pub anchor: String,
    pub heading: String,
    /// Heading level, 1 for `#`.
    pub level: usize,
    pub line: usize,
    pub score: usize,
    pub snippet: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocSearchResult {
    pub query: String,
    pub total: usize,
    pub truncated: bool,
    pub items: Vec<DocSearchItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocOutlineItem {
    pub anchor: String,
    pub heading: String,
    pub level: usize,
    /// Depth in the section tree: 1 for a top section, whatever its heading level.
    pub depth: usize,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocOutlineResult {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<DocMeta>,
    pub truncated: bool,
    pub items: Vec<DocOutlineItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocSectionResult {
    pub path: String,
    pub anchor: String,
    pub heading: String,
    pub line: usize,
    pub end_line: usize,
    pub text: String,
    pub truncated: bool,
}

/// Which way a `doc_links` entry points.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkDirection {
    ToDocs,
    ToCode,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocLinkItem {
    pub direction: LinkDirection,
    /// The documenting file (`to_docs`) or the document (`to_code`).
    pub path: String,
    pub anchor: String,
    pub heading: String,
    /// The referenced file.
    pub target_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_symbol: Option<String>,
    /// `file` or `symbol`.
    pub form: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocLinksResult {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub total: usize,
    pub items: Vec<DocLinkItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SectionRef {
    pub path: String,
    pub anchor: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocTermItem {
    pub term: String,
    pub count: u32,
    pub sections: Vec<SectionRef>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocTermsResult {
    pub path_prefix: String,
    pub total: usize,
    pub truncated: bool,
    pub items: Vec<DocTermItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiSurfaceItem {
    pub path: String,
    /// `operation` or `schema`.
    pub item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiSurfaceResult {
    pub total: usize,
    pub items: Vec<ApiSurfaceItem>,
}

// --- New in 0.4.0 ---

/// A directory in `modules`. Every count is recursive.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleItem {
    pub path: String,
    pub depth: usize,
    pub files: usize,
    pub code_files: usize,
    pub symbols: usize,
    pub doc_files: usize,
    pub doc_sections: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModulesResult {
    pub path_prefix: String,
    pub max_depth: usize,
    pub total: usize,
    pub items: Vec<ModuleItem>,
}

/// One import statement of the `imports` query.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportItem {
    pub path: String,
    pub line: usize,
    pub text: String,
    pub module: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub language: String,
    pub resolved: bool,
}

/// `imports`. `files` and `imports` count every match, not only the listed items.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportsResult {
    pub query: String,
    pub files: usize,
    pub imports: usize,
    pub truncated: bool,
    pub items: Vec<ImportItem>,
}

/// `init`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InitStatus {
    Refreshed,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InitResult {
    pub status: InitStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn symbol(path: &str, name: &str, start: usize, end: usize) -> NodeData {
        NodeData {
            path: path.to_string(),
            name: name.to_string(),
            kind: "function".to_string(),
            line: 1,
            start_byte: start,
            end_byte: end,
            node_type: NodeType::Symbol,
            docstring: None,
            role: None,
        }
    }

    #[test]
    fn error_codes_serialise_as_snake_case() {
        let codes = [
            (ErrorCode::IndexMissing, "index_missing"),
            (ErrorCode::IndexStale, "index_stale"),
            (ErrorCode::NotFound, "not_found"),
            (ErrorCode::InvalidArgument, "invalid_argument"),
            (ErrorCode::IoError, "io_error"),
        ];
        for (code, want) in codes {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(want));
        }
        assert_eq!(
            serde_json::to_value(QueryError::index_missing()).unwrap(),
            json!({"code": "index_missing", "message": "Error: Index not loaded"})
        );
        assert_eq!(QueryError::index_stale().message, STALE_INDEX_MESSAGE);
        assert_eq!(
            QueryError::index_missing().to_string(),
            "Error: Index not loaded"
        );
    }

    #[test]
    fn doc_messages_are_classified() {
        let stale = QueryError::from_doc_message(STALE_INDEX_MESSAGE.to_string());
        assert_eq!(stale.code, ErrorCode::IndexStale);
        let range = QueryError::from_doc_message("Stale range: a.md changed".to_string());
        assert_eq!(range.code, ErrorCode::IndexStale);
        let unknown = QueryError::from_doc_message("Unknown anchor #x in a.md.".to_string());
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert_eq!(unknown.message, "Unknown anchor #x in a.md.");
    }

    #[test]
    fn index_info_keeps_nulls_for_a_missing_index() {
        assert_eq!(
            serde_json::to_value(IndexInfo::missing()).unwrap(),
            json!({
                "state": "missing", "schema_version": null, "commit": null,
                "dirty": null, "stale": null
            })
        );
        let meta = IndexMeta {
            schema_version: 3,
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(IndexInfo::loaded(Some(&meta), false)).unwrap(),
            json!({
                "state": "ok", "schema_version": 3, "commit": null,
                "dirty": null, "stale": false
            })
        );
        let stale = serde_json::to_value(IndexInfo::loaded(None, true)).unwrap();
        assert_eq!(stale["state"], json!("stale"));
        assert_eq!(stale["stale"], json!(true));
    }

    #[test]
    fn absent_optional_fields_are_omitted_not_null() {
        let item = SymbolItem {
            symbol: "load".into(),
            kind: "function".into(),
            node_type: NodeType::Symbol,
            path: "a.rs".into(),
            line: 3,
            end_line: None,
            role: None,
            preview: None,
        };
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            json!({"symbol": "load", "kind": "function", "node_type": "symbol",
                   "path": "a.rs", "line": 3})
        );
        let with = SymbolItem {
            end_line: Some(9),
            role: Some("Producer".into()),
            preview: Some("```rs\n```".into()),
            node_type: NodeType::KafkaTopic,
            ..item
        };
        let v = serde_json::to_value(&with).unwrap();
        assert_eq!(v["node_type"], json!("kafka_topic"));
        assert_eq!(v["end_line"], json!(9));
        assert_eq!(v["role"], json!("Producer"));
        assert!(v["preview"].is_string());
    }

    #[test]
    fn blast_radius_always_carries_its_counts() {
        let result = BlastRadiusResult {
            target_path: "a.rs".into(),
            target_symbol: None,
            target_line: None,
            matched: BlastMatch::File,
            via: None,
            direct: 0,
            transitive: 0,
            total: 0,
            files_affected: 0,
            languages: vec![],
            truncated: false,
            items: vec![],
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({"target_path": "a.rs", "matched": "file", "direct": 0, "transitive": 0,
                   "total": 0, "files_affected": 0, "languages": [],
                   "truncated": false, "items": []})
        );
        let found = BlastRadiusResult {
            target_symbol: Some("load".into()),
            target_line: Some(4),
            matched: BlastMatch::Symbol,
            via: None,
            direct: 1,
            transitive: 0,
            total: 1,
            files_affected: 1,
            languages: vec!["rust".into()],
            ..result
        };
        let v = serde_json::to_value(&found).unwrap();
        assert_eq!(v["matched"], json!("symbol"));
        assert_eq!(v["languages"], json!(["rust"]));
        assert_eq!(v["direct"], json!(1));
    }

    #[test]
    fn plain_enums_serialise_as_snake_case() {
        assert_eq!(
            serde_json::to_value(LinkDirection::ToDocs).unwrap(),
            json!("to_docs")
        );
        assert_eq!(
            serde_json::to_value(LinkDirection::ToCode).unwrap(),
            json!("to_code")
        );
        assert_eq!(
            serde_json::to_value(InitStatus::Refreshed).unwrap(),
            json!("refreshed")
        );
        assert_eq!(
            serde_json::to_value(BlastMatch::NotFound).unwrap(),
            json!("not_found")
        );
    }

    #[test]
    fn tree_leaves_have_no_children_field() {
        let tree = TreeNode {
            name: "src".into(),
            children: vec![TreeNode {
                name: "main.rs".into(),
                children: vec![],
            }],
        };
        assert_eq!(
            serde_json::to_value(&tree).unwrap(),
            json!({"name": "src", "children": [{"name": "main.rs"}]})
        );
    }

    #[test]
    fn nested_symbols_are_qualified_by_their_enclosing_symbols() {
        let nodes = [
            symbol("a.py", "Graph", 0, 100),
            symbol("a.py", "load", 10, 40),
            symbol("a.py", "inner", 15, 30),
            symbol("a.py", "after", 100, 120),
            symbol("b.py", "load", 10, 40),
        ];
        let namer = QualifiedNamer::new(nodes.iter());
        assert_eq!(namer.qualify(&nodes[0]), "Graph");
        assert_eq!(namer.qualify(&nodes[1]), "Graph.load");
        assert_eq!(namer.qualify(&nodes[2]), "Graph.load.inner");
        // Adjacent, not nested.
        assert_eq!(namer.qualify(&nodes[3]), "after");
        // Another file's ranges do not enclose it.
        assert_eq!(namer.qualify(&nodes[4]), "load");
    }

    #[test]
    fn qualified_names_are_not_unique() {
        // Two `impl From<..> for E` blocks: both nodes are named E, each holding `from`.
        let nodes = [
            symbol("e.rs", "E", 0, 50),
            symbol("e.rs", "from", 10, 45),
            symbol("e.rs", "E", 60, 110),
            symbol("e.rs", "from", 70, 105),
        ];
        let namer = QualifiedNamer::new(nodes.iter());
        assert_eq!(namer.qualify(&nodes[1]), "E.from");
        assert_eq!(namer.qualify(&nodes[3]), "E.from");
    }

    #[test]
    fn equal_ranges_and_non_symbols_do_not_qualify() {
        let mut file = symbol("a.rs", "a.rs", 0, 200);
        file.node_type = NodeType::File;
        let mut topic = symbol("a.rs", "orders", 20, 30);
        topic.node_type = NodeType::KafkaTopic;
        let same_a = symbol("a.rs", "alias", 5, 15);
        let same_b = symbol("a.rs", "other", 5, 15);
        let nodes = [file, topic, same_a, same_b];
        let namer = QualifiedNamer::new(nodes.iter());
        assert_eq!(namer.qualify(&nodes[1]), "orders");
        assert_eq!(namer.qualify(&nodes[2]), "alias");
        assert_eq!(namer.qualify(&nodes[3]), "other");
    }

    #[test]
    fn end_line_follows_the_byte_range() {
        let src = "fn one() {}\nfn two() {\n    1\n}\nfn three() {}\n";
        let start = src.find("fn two").unwrap();
        let end = src.find("fn three").unwrap();
        // The range ends after the line break following `}`; that break is not a new line.
        assert_eq!(end_line_of(src, 2, start, end), 4);
        // A one-line node.
        assert_eq!(end_line_of(src, 1, 0, 11), 1);
        // Without the trailing break.
        assert_eq!(end_line_of(src, 2, start, end - 1), 4);
        // Windows line endings.
        let crlf = "a\r\nb\r\n";
        assert_eq!(end_line_of(crlf, 1, 0, crlf.len()), 2);
    }

    #[test]
    fn end_line_falls_back_to_the_start_line_when_the_range_does_not_fit() {
        let src = "short\n";
        assert_eq!(end_line_of(src, 7, 0, 999), 7);
        assert_eq!(end_line_of(src, 7, 3, 3), 7);
        // Inside a multi-byte character.
        let utf = "é\nx\n";
        assert_eq!(end_line_of(utf, 1, 1, 4), 1);
    }
}
