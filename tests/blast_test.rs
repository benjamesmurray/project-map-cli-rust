//! Task 3.4: the breadth-first blast radius with depths and counts.

mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::{BlastMatch, BlastRadiusResult};
use project_map_cli_rust::core::toon::ToonFormatter;
use std::path::Path;
use tempfile::{tempdir, TempDir};

fn engine_over(root: &Path, base: &Path) -> QueryEngine {
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &base.join("out")).unwrap();
    let index = base.join("index.json");
    orch.save_index(&index).unwrap();
    QueryEngine::load(&index).unwrap()
}

fn engine_for(fixture: &str) -> (TempDir, QueryEngine) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(fixture, &root);
    let engine = engine_over(&root, dir.path());
    (dir, engine)
}

/// A small tree written file by file.
fn engine_with(files: &[(&str, &str)]) -> (TempDir, QueryEngine) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    for (path, text) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    let engine = engine_over(&root, dir.path());
    (dir, engine)
}

fn depths(result: &BlastRadiusResult) -> Vec<(&str, usize)> {
    result
        .items
        .iter()
        .map(|i| (i.path.as_str(), i.depth))
        .collect()
}

#[test]
fn the_diamond_reports_each_node_once_at_its_shortest_depth() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let r = engine.blast_result("diamond/target.ts", "target", None);
    // `a` is reachable at depth 1 (it imports target) and depth 3 (via m2 and m1).
    assert_eq!(
        depths(&r),
        [
            ("diamond/a.ts", 1),
            ("diamond/m1.ts", 1),
            ("diamond/m2.ts", 2)
        ]
    );
    assert_eq!(r.matched, BlastMatch::Symbol);
    assert_eq!(r.target_symbol.as_deref(), Some("target"));
}

#[test]
fn summary_counts_cover_every_dependent() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let r = engine.blast_result("diamond/target.ts", "target", None);
    assert_eq!(
        (r.direct, r.transitive, r.total, r.files_affected),
        (2, 1, 3, 3)
    );
    assert_eq!(r.languages, ["typescript"]);
    assert_eq!(r.target_path, "diamond/target.ts");
    assert!(r.target_line.is_some());

    // A limit cuts the list, not the counts.
    let cut = engine.blast_result("diamond/target.ts", "target", Some(1));
    assert!(cut.truncated);
    assert_eq!(cut.items.len(), 1);
    assert_eq!(
        (cut.direct, cut.transitive, cut.total, cut.files_affected),
        (2, 1, 3, 3)
    );
    assert_eq!(cut.items[0].path, "diamond/a.ts");
    // The summary fields are always present.
    let v = serde_json::to_value(&cut).unwrap();
    for field in [
        "direct",
        "transitive",
        "total",
        "files_affected",
        "languages",
    ] {
        assert!(v.get(field).is_some(), "{field}");
    }
    assert!(v["items"][0]["depth"] == 1);
}

#[test]
fn the_targets_file_and_enclosing_symbols_are_never_listed() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    // `from` is a method inside an `impl`: its enclosing symbol and file are ancestors.
    let r = engine.blast_result("rust_crate/src/errors.rs", "from", None);
    assert!(
        r.items.iter().all(|i| i.path != "rust_crate/src/errors.rs"),
        "{:?}",
        r.items
    );
    let nodes = engine.check_blast_radius("diamond/target.ts", "target");
    assert!(nodes.iter().all(|n| n.path != "diamond/target.ts"));
    // The same holds for the TOON listing.
    let (radius, nodes) = engine.blast_with_nodes("diamond/target.ts", "target");
    let text = ToonFormatter::format_blast_radius("diamond/target.ts", "target", &nodes, &radius);
    assert!(!text.contains("- diamond/target.ts"), "{text}");
}

#[test]
fn importers_of_the_file_and_of_the_symbol_are_both_depth_one() {
    let (_dir, engine) = engine_with(&[
        (
            "Cargo.toml",
            "[package]\nname = \"mini\"\nversion = \"0.1.0\"\n",
        ),
        ("src/lib.rs", "pub mod a;\npub mod x;\npub mod y;\n"),
        ("src/a.rs", "pub struct S;\n"),
        ("src/x.rs", "use crate::a;\n"),
        ("src/y.rs", "use crate::a::S;\n"),
    ]);
    let r = engine.blast_result("src/a.rs", "S", None);
    assert_eq!(depths(&r), [("src/x.rs", 1), ("src/y.rs", 1)], "{r:?}");
    assert_eq!((r.direct, r.transitive, r.files_affected), (2, 0, 2));
    assert_eq!(r.languages, ["rust"]);
}

#[test]
fn a_symbol_not_in_the_file_falls_back_to_the_file() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let sym = engine.blast_result("diamond/target.ts", "target", None);
    let file = engine.blast_result("./diamond/target.ts", "nope", None);
    assert_eq!(file.matched, BlastMatch::File);
    assert_eq!(file.target_symbol, None);
    assert_eq!(file.target_path, "diamond/target.ts");
    assert_eq!(depths(&file), depths(&sym));
    assert_eq!(file.direct, 2);
    let v = serde_json::to_value(&file).unwrap();
    assert!(v.get("target_symbol").is_none());

    let none = engine.blast_result("diamond/missing.ts", "x", None);
    assert_eq!(none.matched, BlastMatch::NotFound);
    assert_eq!(
        (
            none.direct,
            none.transitive,
            none.total,
            none.files_affected
        ),
        (0, 0, 0, 0)
    );
    assert!(none.languages.is_empty());
}

#[test]
fn a_file_target_is_seeded_with_the_symbols_it_defines() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    // `use crate::a::b::Sym` links to the symbol, not the file. A file-level target
    // (no such symbol in the file) still finds those importers, at depth 1.
    let sym = engine.blast_result("rust_crate/src/a/b.rs", "Sym", None);
    let file = engine.blast_result("rust_crate/src/a/b.rs", "no_such_symbol", None);
    assert_eq!(file.matched, BlastMatch::File);
    assert_eq!(file.target_symbol, None);
    for (path, depth) in depths(&sym) {
        assert!(
            depths(&file).contains(&(path, depth)),
            "{path} missing from the file-level radius"
        );
    }
    assert!(file.direct >= sym.direct);
    assert!(file.items.iter().all(|i| i.path != "rust_crate/src/a/b.rs"));
}

#[test]
fn rust_crate_importers_appear_through_use_crate() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let sym = engine.blast_result("rust_crate/src/a/b.rs", "Sym", None);
    let paths: Vec<_> = depths(&sym);
    assert_eq!(
        paths,
        [
            ("rust_crate/src/a/mod.rs", 1),
            ("rust_crate/src/user.rs", 1),
            ("rust_crate/tests/uses_crate_name.rs", 1)
        ]
    );
    assert!(sym.items.iter().all(|i| i.path != "rust_crate/src/a/b.rs"));
    assert_eq!(sym.languages, ["rust"]);

    // `Gamma` is imported directly, and by `user.rs` through the module `b`.
    let gamma = engine.blast_result("rust_crate/src/c/mod.rs", "Gamma", None);
    assert_eq!(
        depths(&gamma),
        [
            ("rust_crate/src/a/b.rs", 1),
            ("rust_crate/src/a/mod.rs", 1),
            ("rust_crate/src/user.rs", 2)
        ]
    );
    assert_eq!((gamma.direct, gamma.transitive), (2, 1));
}

#[test]
fn languages_list_every_language_of_the_affected_files_sorted() {
    let (_dir, engine) = engine_with(&[
        ("util.ts", "export function util(): number { return 1 }\n"),
        (
            "j.js",
            "import { util } from \"./util\";\nexport const j = util();\n",
        ),
        ("main.py", "import util\nprint(util)\n"),
    ]);
    let r = engine.blast_result("util.ts", "util", None);
    assert_eq!(r.files_affected, 2, "{r:?}");
    assert_eq!(r.languages, ["python", "typescript"]);
}

#[test]
fn doc_nodes_are_not_dependents() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    let r = engine.blast_result("src/audit/log.rs", "append", None);
    assert!(r.items.iter().all(|i| !i.path.ends_with(".md")), "{r:?}");
    assert!(r.items.iter().all(|i| !i.node_type.is_doc()));
}

#[test]
fn a_topic_does_not_list_its_own_file() {
    let (_dir, engine) = engine_for("tests/fixtures/streaming");
    let r = engine.blast_result("producer.py", "wde.labels.phase.v1", None);
    assert_eq!(r.matched, BlastMatch::Symbol);
    assert_eq!((r.total, r.files_affected), (0, 0), "{r:?}");
}

#[test]
fn toon_summary_reports_direct_and_transitive_counts() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let (radius, nodes) = engine.blast_with_nodes("diamond/target.ts", "target");
    let text = ToonFormatter::format_blast_radius("diamond/target.ts", "target", &nodes, &radius);
    assert!(text.contains("- Total Impacted Nodes: 3\n"), "{text}");
    assert!(
        text.contains("- Direct Dependents: 2 | Transitive: 1\n"),
        "{text}"
    );
    assert!(text.contains("- Unique Files Affected: 3\n"), "{text}");
    // Per-node lines keep their format.
    assert!(
        text.contains("- diamond/a.ts (ln: 0) -> diamond.a\n"),
        "{text}"
    );

    let (empty, nodes) = engine.blast_with_nodes("diamond/a.ts", "a");
    let text = ToonFormatter::format_blast_radius("diamond/a.ts", "a", &nodes, &empty);
    assert!(text.contains("No dependent components found."), "{text}");
}

#[test]
fn plan_reports_depth_and_its_target() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let plan = engine.plan_result("diamond.a", None);
    assert_eq!(plan.target_path.as_deref(), Some("diamond/a.ts"));
    assert!(plan.total > 0);
    assert!(plan.items.iter().all(|i| i.depth >= 1));
    assert!(plan.items.windows(2).all(|w| w[0].depth <= w[1].depth));
    let v = serde_json::to_value(&plan).unwrap();
    assert!(v["items"][0]["depth"].is_number());
    let again = engine.analyze_impact("diamond.a");
    assert_eq!(again.len(), plan.total);
}
