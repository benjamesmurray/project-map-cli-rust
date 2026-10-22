mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::*;
use serde_json::Value;
use std::path::Path;
use tempfile::{tempdir, TempDir};

const DOCS: &str = "tests/fixtures/docs";

/// An engine over a copy of a fixture, built with an absolute root so indexed paths
/// are root-relative and text is read from the copy.
fn engine_for(fixture: &str) -> (TempDir, QueryEngine) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(fixture, &root);
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &dir.path().join("out")).unwrap();
    let index = dir.path().join("index.json");
    orch.save_index(&index).unwrap();
    (dir, QueryEngine::load(&index).unwrap())
}

/// No `null` anywhere: a field that does not apply is omitted.
fn assert_no_nulls(value: &Value, what: &str) {
    match value {
        Value::Null => panic!("{what}: null in {value}"),
        Value::Array(a) => a.iter().for_each(|v| assert_no_nulls(v, what)),
        Value::Object(o) => o.values().for_each(|v| assert_no_nulls(v, what)),
        _ => {}
    }
}

fn json<T: serde::Serialize>(what: &str, result: &T) -> Value {
    let v = serde_json::to_value(result).unwrap();
    assert_no_nulls(&v, what);
    v
}

#[test]
fn entity_list_matches_find_symbols_and_qualifies_names() {
    let (_dir, engine) = engine_for(DOCS);
    let nodes = engine.find_symbols("append");
    let list = engine.entity_list("append", Some(3), None);
    assert_eq!(list.total, nodes.len());
    assert!(!list.truncated);
    assert_eq!(
        list.items
            .iter()
            .map(|i| (&i.path, i.line))
            .collect::<Vec<_>>(),
        nodes.iter().map(|n| (&n.path, n.line)).collect::<Vec<_>>(),
        "same nodes in the same order"
    );

    let method = list
        .items
        .iter()
        .find(|i| i.path == "src/audit/log.rs" && i.symbol.ends_with("append"))
        .expect("AuditLog::append");
    assert_eq!(method.symbol, "AuditLog.append");
    assert_eq!((method.line, method.end_line), (11, Some(13)));
    assert!(method.preview.as_deref().unwrap().contains("```rs"));
    json("entity_list", &list);

    // No preview asked for, none given.
    let bare = engine.entity_list("append", None, None);
    assert!(bare.items.iter().all(|i| i.preview.is_none()));
}

#[test]
fn entity_list_limit_reports_the_total_and_truncation() {
    let (_dir, engine) = engine_for(DOCS);
    let all = engine.entity_list("a", None, None).total;
    assert!(all > 2);
    let cut = engine.entity_list("a", None, Some(2));
    assert_eq!((cut.total, cut.items.len(), cut.truncated), (all, 2, true));
    let exact = engine.entity_list("a", None, Some(all));
    assert!(!exact.truncated);
    assert!(engine
        .entity_list("zzzznothing", None, None)
        .items
        .is_empty());
}

#[test]
fn outline_result_lists_a_file_and_rejects_an_unindexed_one() {
    let (_dir, engine) = engine_for(DOCS);
    let outline = engine.outline_result("./src/audit/log.rs").unwrap();
    assert_eq!(outline.path, "src/audit/log.rs");
    let append = outline.items.iter().find(|i| i.symbol == "append");
    assert!(append.is_some() || outline.items.iter().any(|i| i.symbol == "AuditLog"));
    assert!(outline
        .items
        .iter()
        .all(|i| i.end_line.is_some_and(|e| e >= i.line)));
    json("outline", &outline);

    let err = engine.outline_result("src/nope.rs").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(err.message, "File not indexed: src/nope.rs");
}

#[test]
fn outline_result_marks_imports_with_their_role_and_path() {
    let (_dir, engine) = engine_for("tests/fixtures/python");
    let outline = engine.outline_result("module_a.py").unwrap();
    let imports: Vec<_> = outline
        .items
        .iter()
        .filter(|i| i.role.as_deref() == Some("Import"))
        .collect();
    assert!(!imports.is_empty(), "module_a imports module_b");
    assert!(imports
        .iter()
        .all(|i| i.path.is_some() && i.end_line.is_none()));
    json("outline imports", &outline);
}

#[test]
fn files_result_lists_paths() {
    let (_dir, engine) = engine_for(DOCS);
    let files = engine.files_result("audit", None);
    assert!(files.total >= 2);
    assert!(files.paths.iter().all(|p| p.contains("audit")));
    let cut = engine.files_result("audit", Some(1));
    assert_eq!(
        (cut.total, cut.paths.len(), cut.truncated),
        (files.total, 1, true)
    );
}

#[test]
fn fetch_result_returns_the_source_and_its_lines() {
    let (_dir, engine) = engine_for(DOCS);
    let fetched = engine.fetch_result("src/audit/log.rs", "append").unwrap();
    assert_eq!((fetched.line, fetched.end_line), (11, 13));
    assert_eq!(fetched.symbol, "AuditLog.append");
    assert!(fetched.source.starts_with("pub fn append"));
    assert!(fetched.source.trim_end().ends_with('}'));
    json("fetch", &fetched);

    let missing = engine.fetch_result("src/audit/log.rs", "nope").unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
    assert_eq!(
        missing.message,
        "Symbol not found: nope in src/audit/log.rs"
    );
}

#[test]
fn blast_result_agrees_with_the_walk_and_names_its_target() {
    let (_dir, engine) = engine_for(DOCS);
    let nodes = engine.check_blast_radius("src/audit/log.rs", "append");
    let result = engine.blast_result("src/audit/log.rs", "append", None);
    assert_eq!(result.matched, BlastMatch::Symbol);
    assert_eq!(result.target_symbol.as_deref(), Some("AuditLog.append"));
    assert_eq!(result.target_line, Some(11));
    assert_eq!(result.total, nodes.len());
    assert_eq!(
        result
            .items
            .iter()
            .map(|i| (&i.path, i.line))
            .collect::<Vec<_>>(),
        nodes.iter().map(|n| (&n.path, n.line)).collect::<Vec<_>>()
    );
    json("blast", &result);

    // A symbol that is not in the file falls back to the file.
    let file = engine.blast_result("./src/audit/log.rs", "nope", None);
    assert_eq!(file.matched, BlastMatch::File);
    assert_eq!(file.target_symbol, None);
    assert_eq!(file.target_path, "src/audit/log.rs");
    json("blast file", &file);

    let none = engine.blast_result("src/missing.rs", "x", None);
    assert_eq!(none.matched, BlastMatch::NotFound);
    assert_eq!((none.total, none.target_line), (0, None));
}

#[test]
fn blast_result_limit_keeps_the_total() {
    let (_dir, engine) = engine_for("tests/fixtures/python");
    let all = engine.blast_result("module_b.py", "hello", None);
    if all.total > 0 {
        let cut = engine.blast_result("module_b.py", "hello", Some(0));
        assert_eq!(
            (cut.total, cut.items.len(), cut.truncated),
            (all.total, 0, true)
        );
    }
}

#[test]
fn plan_result_agrees_with_analyze_impact() {
    let (_dir, engine) = engine_for(DOCS);
    let nodes = engine.analyze_impact("append");
    let plan = engine.plan_result("append", None);
    assert_eq!(plan.requested, "append");
    assert_eq!(plan.target_path.as_deref(), Some("src/audit/log.rs"));
    assert_eq!(plan.total, nodes.len());
    json("plan", &plan);
    let unknown = engine.plan_result("zzzznothing", None);
    assert_eq!((unknown.total, unknown.target_path), (0, None));
}

#[test]
fn status_result_mirrors_the_index_meta_and_cuts_the_tree() {
    let (_dir, engine) = engine_for(DOCS);
    let meta = engine.meta().unwrap().clone();
    let status = engine.status_result(1, vec!["feature-a".into()]);
    assert_eq!(status.files, meta.file_count);
    assert_eq!(status.symbols, meta.symbol_count);
    assert_eq!(status.doc_sections, meta.section_count);
    assert_eq!(status.schema_version, meta.schema_version);
    assert!(!status.stale);
    assert_eq!(status.active_features, ["feature-a"]);
    assert!(status.tree.children.iter().all(|c| c.children.is_empty()));
    assert!(!status.tree.children.is_empty());

    let deep = engine.status_result(9, vec![]);
    assert!(deep.tree.children.iter().any(|c| !c.children.is_empty()));
    let v = serde_json::to_value(&status).unwrap();
    assert!(
        v["commit"].is_null() && v["at_head"].is_null(),
        "Git fields are null, not omitted"
    );
    assert!(v.get("git_error").is_none());
}

#[test]
fn doc_results_carry_levels_lines_and_scores() {
    let (_dir, engine) = engine_for(DOCS);

    let search = engine.doc_search_result("audit log", 2).unwrap();
    assert_eq!(
        (search.items.len(), search.total, search.truncated),
        (2, 6, true)
    );
    assert!(search
        .items
        .iter()
        .all(|h| h.level >= 1 && h.line >= 1 && h.score > 0));
    json("doc_search", &search);

    let outline = engine.doc_outline_result("docs/guide.md", None).unwrap();
    assert!(outline
        .items
        .iter()
        .all(|e| e.end_line >= e.line && e.depth >= 1));
    let skipped = outline
        .items
        .iter()
        .find(|e| e.anchor == "skipped-from-h1")
        .expect("h3 directly under an h1");
    assert_eq!((skipped.level, skipped.depth), (3, 2));
    json("doc_outline", &outline);

    let section = engine
        .doc_section_result("docs/guide.md", "usage", None)
        .unwrap();
    assert!(section.end_line >= section.line && !section.truncated);
    let cut = engine
        .doc_section_result("docs/guide.md", "operator-guide", Some(20))
        .unwrap();
    assert!(cut.truncated && cut.text.chars().count() == 20);
    json("doc_section", &section);

    let to_docs = engine
        .doc_links_result("src/audit/log.rs", Some("append"))
        .unwrap();
    assert!(to_docs
        .items
        .iter()
        .all(|i| i.direction == LinkDirection::ToDocs));
    assert!(to_docs
        .items
        .iter()
        .all(|i| i.target_symbol.as_deref() == Some("append") && i.form == "symbol"));
    assert_eq!(to_docs.total, to_docs.items.len());
    json("doc_links to_docs", &to_docs);
    let to_code = engine
        .doc_links_result("docs/adr/004-immutable-audit-log.md", None)
        .unwrap();
    assert!(to_code
        .items
        .iter()
        .all(|i| i.direction == LinkDirection::ToCode));
    assert!(!to_code.items.is_empty());
    assert!(to_code
        .items
        .iter()
        .all(|i| (i.form == "symbol") == i.target_symbol.is_some()));
    json("doc_links to_code", &to_code);

    let terms = engine.doc_terms_result(None, 2).unwrap();
    assert!(terms.truncated && terms.items.len() == 2);
    json("doc_terms", &terms);

    let api = engine.api_surface_result(None).unwrap();
    let get = api
        .items
        .iter()
        .find(|i| i.operation_id.as_deref() == Some("listEntries"))
        .unwrap();
    assert_eq!(
        (get.method.as_deref(), get.route.as_deref()),
        (Some("GET"), Some("/entries"))
    );
    assert!(api
        .items
        .iter()
        .any(|i| i.item == "schema" && i.name.as_deref() == Some("AuditEntry")));
    assert!(api.items.iter().all(|i| i.line >= 1));
    json("api_surface", &api);
}

#[test]
fn doc_result_errors_carry_codes() {
    let (_dir, engine) = engine_for(DOCS);
    for err in [
        engine.doc_outline_result("nope.md", None).unwrap_err(),
        engine
            .doc_section_result("docs/guide.md", "nope", None)
            .unwrap_err(),
        engine.doc_links_result("src/missing.rs", None).unwrap_err(),
        engine
            .api_surface_result(Some("config/settings.yaml"))
            .unwrap_err(),
    ] {
        assert_eq!(err.code, ErrorCode::NotFound, "{err}");
    }

    let dir = tempdir().unwrap();
    let index = dir.path().join("index.json");
    std::fs::copy("tests/fixtures/compatibility/v0_2_0_index.json", &index).unwrap();
    let old = QueryEngine::load(&index).unwrap();
    assert_eq!(old.index_info().state, IndexState::Stale);
    for err in [
        old.doc_search_result("audit", 10).unwrap_err(),
        old.doc_outline_result("README.md", None).unwrap_err(),
        old.doc_links_result("src/main.rs", None).unwrap_err(),
        old.doc_terms_result(None, 3).unwrap_err(),
        old.api_surface_result(None).unwrap_err(),
    ] {
        assert_eq!(err, QueryError::index_stale());
    }
}

#[test]
fn index_info_describes_a_current_index() {
    let (_dir, engine) = engine_for(DOCS);
    let info = engine.index_info();
    assert_eq!(info.state, IndexState::Ok);
    assert_eq!(info.stale, Some(false));
    assert_eq!(
        info.schema_version,
        Some(engine.meta().unwrap().schema_version)
    );
}

#[test]
fn imports_fixture_results_serialise_without_nulls() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    for query in ["Sym", "target", "Checkout", "from", "Gamma"] {
        json(query, &engine.entity_list(query, None, None));
    }
    json(
        "blast",
        &engine.blast_result("diamond/target.ts", "target", None),
    );
    assert!(Path::new(IMPORTS_FIXTURE).exists());
}
