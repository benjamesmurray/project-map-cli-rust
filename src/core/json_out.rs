//! The JSON renderer: wraps a typed result (`core::results`) in the contract
//! envelope of the Specification (Data Models).
//!
//! ```json
//! { "tool": "...", "contract": 1, "index": {..}, "truncated": false,
//!   "total": 3, "summary": {..}, "results": [..] }
//! ```
//!
//! An error has `error: {code, message}` and no `summary` or `results`. JSON is never
//! cut by characters: lists are capped by the tools' item limits only, and
//! `truncated` and `total` say what was left out.

use crate::core::results::*;
use serde::Serialize;
use serde_json::{json, Map, Value};

/// The JSON contract version. A change to the envelope, a field name or an error code
/// is breaking and raises it.
pub const CONTRACT_VERSION: u32 = 1;

/// The parts of a result the envelope places apart from the item list.
pub struct Payload {
    pub truncated: bool,
    /// Matches before any limit.
    pub total: usize,
    pub summary: Map<String, Value>,
    pub results: Vec<Value>,
}

/// A typed result that fits the envelope.
pub trait ToPayload {
    fn payload(&self) -> Payload;
}

fn object<T: Serialize>(value: &T) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn items<T: Serialize>(items: &[T]) -> Vec<Value> {
    items
        .iter()
        .filter_map(|i| serde_json::to_value(i).ok())
        .collect()
}

/// The named fields of `value`, in the order given, skipping absent ones.
fn pick<T: Serialize>(value: &T, keys: &[&str]) -> Map<String, Value> {
    let mut all = object(value);
    let mut out = Map::new();
    for key in keys {
        if let Some(v) = all.remove(*key) {
            out.insert((*key).to_string(), v);
        }
    }
    out
}

impl ToPayload for StatusResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: 0,
            summary: object(self),
            results: Vec::new(),
        }
    }
}

impl ToPayload for EntityList {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.total,
            summary: Map::new(),
            results: items(&self.items),
        }
    }
}

impl ToPayload for FileOutlineResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: false,
            total: self.items.len(),
            summary: pick(self, &["path"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for FileList {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.total,
            summary: Map::new(),
            results: self.paths.iter().map(|p| json!({ "path": p })).collect(),
        }
    }
}

impl ToPayload for FetchResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: false,
            total: 1,
            summary: object(self),
            results: Vec::new(),
        }
    }
}

impl ToPayload for BlastRadiusResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.total,
            summary: pick(
                self,
                &[
                    "target_path",
                    "target_symbol",
                    "target_line",
                    "matched",
                    "direct",
                    "transitive",
                    "total",
                    "files_affected",
                    "languages",
                ],
            ),
            results: items(&self.items),
        }
    }
}

impl ToPayload for PlanResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.total,
            summary: pick(
                self,
                &["requested", "target_symbol", "target_path", "target_line"],
            ),
            results: items(&self.items),
        }
    }
}

impl ToPayload for DocSearchResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.total,
            summary: pick(self, &["query"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for DocOutlineResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.items.len(),
            summary: pick(self, &["path", "frontmatter"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for DocSectionResult {
    fn payload(&self) -> Payload {
        let mut summary = object(self);
        summary.remove("truncated");
        Payload {
            truncated: self.truncated,
            total: 1,
            summary,
            results: Vec::new(),
        }
    }
}

impl ToPayload for DocLinksResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: false,
            total: self.total,
            summary: pick(self, &["path", "symbol", "total"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for DocTermsResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.total,
            summary: pick(self, &["path_prefix"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for ApiSurfaceResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: false,
            total: self.total,
            summary: pick(self, &["total"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for ModulesResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: false,
            total: self.total,
            summary: pick(self, &["path_prefix", "max_depth"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for ImportsResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: self.truncated,
            total: self.imports,
            summary: pick(self, &["query", "files", "imports"]),
            results: items(&self.items),
        }
    }
}

impl ToPayload for InitResult {
    fn payload(&self) -> Payload {
        Payload {
            truncated: false,
            total: 0,
            summary: object(self),
            results: Vec::new(),
        }
    }
}

/// The envelope for one tool call. `index` describes the index the call ran against
/// (`IndexInfo::missing()` when none was loaded).
pub fn to_json<T: ToPayload>(
    tool: &str,
    result: &Result<T, QueryError>,
    index: &IndexInfo,
) -> Value {
    let mut envelope = Map::new();
    envelope.insert("tool".into(), json!(tool));
    envelope.insert("contract".into(), json!(CONTRACT_VERSION));
    envelope.insert(
        "index".into(),
        serde_json::to_value(index).unwrap_or(Value::Null),
    );
    match result {
        Ok(r) => {
            let p = r.payload();
            envelope.insert("truncated".into(), json!(p.truncated));
            envelope.insert("total".into(), json!(p.total));
            envelope.insert("summary".into(), Value::Object(p.summary));
            envelope.insert("results".into(), Value::Array(p.results));
        }
        Err(e) => {
            envelope.insert(
                "error".into(),
                json!({ "code": e.code, "message": e.message }),
            );
        }
    }
    Value::Object(envelope)
}

/// The envelope of a call that failed before any result existed (an invalid argument,
/// a missing index).
pub fn error_json(tool: &str, error: &QueryError, index: &IndexInfo) -> Value {
    to_json::<InitResult>(tool, &Err(error.clone()), index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::NodeType;

    fn index() -> IndexInfo {
        IndexInfo::loaded(None, false)
    }

    #[test]
    fn a_result_fills_every_envelope_field() {
        let r = Ok(ModulesResult {
            path_prefix: ".".into(),
            max_depth: 2,
            total: 1,
            items: vec![ModuleItem {
                path: ".".into(),
                depth: 0,
                files: 1,
                code_files: 1,
                symbols: 2,
                doc_files: 0,
                doc_sections: 0,
            }],
        });
        let v = to_json("modules", &r, &index());
        assert_eq!(v["tool"], "modules");
        assert_eq!(v["contract"], CONTRACT_VERSION);
        assert_eq!(v["index"]["state"], "ok");
        assert_eq!(v["truncated"], false);
        assert_eq!(v["total"], 1);
        assert_eq!(v["summary"], json!({"path_prefix": ".", "max_depth": 2}));
        assert_eq!(v["results"][0]["symbols"], 2);
    }

    #[test]
    fn an_error_has_no_results() {
        let r: Result<EntityList, _> = Err(QueryError::index_missing());
        let v = to_json("search", &r, &IndexInfo::missing());
        assert_eq!(v["error"]["code"], "index_missing");
        assert_eq!(v["error"]["message"], "Error: Index not loaded");
        assert_eq!(v["index"]["state"], "missing");
        assert!(v.get("results").is_none());
        assert!(v.get("summary").is_none());
        assert_eq!(
            v,
            error_json(
                "search",
                &QueryError::index_missing(),
                &IndexInfo::missing()
            )
        );
    }

    #[test]
    fn blast_summary_keeps_its_counts_beside_the_items() {
        let r = Ok(BlastRadiusResult {
            target_path: "a.rs".into(),
            target_symbol: Some("load".into()),
            target_line: Some(3),
            matched: BlastMatch::Symbol,
            direct: 1,
            transitive: 0,
            total: 1,
            files_affected: 1,
            languages: vec!["rust".into()],
            truncated: false,
            items: vec![BlastItem {
                symbol: "main".into(),
                kind: "function".into(),
                node_type: NodeType::Symbol,
                path: "b.rs".into(),
                line: 1,
                end_line: Some(2),
                depth: 1,
            }],
        });
        let v = to_json("blast_radius", &r, &index());
        assert_eq!(v["total"], 1);
        assert_eq!(v["summary"]["direct"], 1);
        assert_eq!(v["summary"]["matched"], "symbol");
        assert!(v["summary"].get("items").is_none());
        assert!(v["summary"].get("truncated").is_none());
        assert_eq!(v["results"][0]["depth"], 1);
    }

    #[test]
    fn doc_section_text_is_whole_and_truncated_moves_to_the_envelope() {
        let text = "x".repeat(20_000);
        let r = Ok(DocSectionResult {
            path: "a.md".into(),
            anchor: "a".into(),
            heading: "A".into(),
            line: 1,
            end_line: 2,
            text: text.clone(),
            truncated: false,
        });
        let v = to_json("doc_section", &r, &index());
        assert_eq!(v["summary"]["text"].as_str().unwrap().len(), text.len());
        assert!(v["summary"].get("truncated").is_none());
        assert_eq!(v["truncated"], false);
        assert_eq!(v["results"], json!([]));
    }

    #[test]
    fn file_list_wraps_each_path() {
        let r = Ok(FileList {
            query: "a".into(),
            total: 2,
            truncated: true,
            paths: vec!["a.rs".into(), "b/a.rs".into()],
        });
        let v = to_json("query", &r, &index());
        assert_eq!(v["results"], json!([{"path": "a.rs"}, {"path": "b/a.rs"}]));
        assert_eq!(v["truncated"], true);
        assert_eq!(v["total"], 2);
    }
}
