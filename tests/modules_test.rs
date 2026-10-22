//! Task 3.3: `modules_result`, recursive per-directory counts.

mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::{ErrorCode, ModuleItem};
use tempfile::{tempdir, TempDir};

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

fn find<'a>(items: &'a [ModuleItem], path: &str) -> &'a ModuleItem {
    items
        .iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("no module {path} in {items:?}"))
}

fn counts(m: &ModuleItem) -> [usize; 5] {
    [
        m.files,
        m.code_files,
        m.symbols,
        m.doc_files,
        m.doc_sections,
    ]
}

#[test]
fn the_root_counts_match_the_index_meta() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    let meta = engine.meta().unwrap();
    let all = engine.modules_result(None, 99).unwrap();
    let root = find(&all.items, ".");
    assert_eq!(root.depth, 0);
    assert_eq!(root.files, meta.file_count);
    assert_eq!(root.symbols, meta.symbol_count);
    assert_eq!(root.doc_sections, meta.section_count);
    assert_eq!(root.doc_files, meta.doc_file_count);
    assert!(root.code_files > 0 && root.code_files < root.files);
    assert_eq!(all.path_prefix, ".");
    assert_eq!(all.total, all.items.len());
}

#[test]
fn fixture_directories_have_known_counts() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    let all = engine.modules_result(None, 99).unwrap();
    let audit = find(&all.items, "src/audit");
    assert_eq!((audit.files, audit.code_files), (2, 2));
    assert_eq!((audit.doc_files, audit.doc_sections), (0, 0));
    assert!(audit.symbols >= 4);
    assert_eq!(audit.depth, 2);
    let docs = find(&all.items, "docs");
    assert_eq!(docs.code_files, 0);
    assert!(docs.doc_files >= 5 && docs.doc_sections > 10 && docs.symbols == 0);
    // The api directory holds OpenAPI files, which are not Markdown documents.
    let api = find(&all.items, "api");
    assert_eq!((api.doc_files, api.doc_sections, api.code_files), (0, 0, 0));
    assert!(api.files >= 3);
    // `src` includes its subdirectories.
    let src = find(&all.items, "src");
    assert_eq!(
        src.files,
        find(&all.items, "src/audit").files + find(&all.items, "src/billing").files
    );
}

#[test]
fn children_sum_to_the_parent_minus_its_own_files() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    let all = engine.modules_result(None, 99).unwrap();
    let files = engine.get_all_file_paths();
    for parent in &all.items {
        let own_files = files
            .iter()
            .filter(|p| {
                let rel = p.trim_start_matches("./");
                match rel.rsplit_once('/') {
                    Some((dir, _)) => dir == parent.path,
                    None => parent.path == ".",
                }
            })
            .count();
        let children: Vec<&ModuleItem> = all
            .items
            .iter()
            .filter(|c| {
                c.depth == parent.depth + 1
                    && if parent.path == "." {
                        !c.path.contains('/')
                    } else {
                        c.path.starts_with(&format!("{}/", parent.path))
                    }
            })
            .collect();
        assert_eq!(
            parent.files,
            own_files + children.iter().map(|c| c.files).sum::<usize>(),
            "{}",
            parent.path
        );
        // Symbols and sections live in files, so they add up the same way.
        assert!(children.iter().map(|c| c.symbols).sum::<usize>() <= parent.symbols);
    }
}

#[test]
fn max_depth_limits_the_directories_but_not_the_counts() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    let deep = engine.modules_result(None, 99).unwrap();
    let zero = engine.modules_result(None, 0).unwrap();
    assert_eq!(zero.items.len(), 1);
    assert_eq!(zero.items[0].path, ".");
    assert_eq!(counts(&zero.items[0]), counts(find(&deep.items, ".")));

    let one = engine.modules_result(None, 1).unwrap();
    assert!(one
        .items
        .iter()
        .all(|m| m.depth <= 1 && !m.path.contains('/')));
    assert!(one.items.len() > 1);
    // A directory below the cut still counts toward its ancestor.
    assert_eq!(
        counts(find(&one.items, "src")),
        counts(find(&deep.items, "src"))
    );

    let two = engine.modules_result(None, 2).unwrap();
    assert!(two.items.iter().any(|m| m.path == "src/audit"));
    assert!(two.items.iter().all(|m| m.depth <= 2));
    assert_eq!(two.max_depth, 2);
}

#[test]
fn a_prefix_selects_a_subtree_with_depths_relative_to_it() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    let deep = engine.modules_result(None, 99).unwrap();
    for spelled in ["src", "src/", "./src", "./src/"] {
        let src = engine.modules_result(Some(spelled), 1).unwrap();
        assert_eq!(src.path_prefix, "src");
        assert_eq!(
            src.items
                .iter()
                .map(|m| m.path.as_str())
                .collect::<Vec<_>>(),
            ["src", "src/audit", "src/billing"],
            "{spelled}"
        );
        assert_eq!(
            src.items.iter().map(|m| m.depth).collect::<Vec<_>>(),
            [0, 1, 1]
        );
        assert_eq!(counts(&src.items[0]), counts(find(&deep.items, "src")));
    }
    let leaf = engine.modules_result(Some("src/audit"), 3).unwrap();
    assert_eq!(leaf.items.len(), 1);
    assert_eq!(leaf.items[0].depth, 0);
    // A prefix that merely starts with the same letters is not a match.
    let partial = engine.modules_result(Some("sr"), 1).unwrap_err();
    assert_eq!(partial.code, ErrorCode::NotFound);
}

#[test]
fn an_unknown_prefix_or_a_file_is_not_found() {
    let (_dir, engine) = engine_for("tests/fixtures/docs");
    for bad in ["nope", "src/audit/log.rs", "docs/missing"] {
        let err = engine.modules_result(Some(bad), 1).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "{bad}");
        assert!(
            err.message.contains(bad.trim_end_matches('/')),
            "{}",
            err.message
        );
    }
}

#[test]
fn root_files_count_under_a_dot_and_results_are_sorted_by_path() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let all = engine.modules_result(None, 99).unwrap();
    let paths: Vec<&str> = all.items.iter().map(|m| m.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
    assert_eq!(paths[0], ".");
    let direct = engine
        .get_all_file_paths()
        .iter()
        .filter(|p| !p.trim_start_matches("./").contains('/'))
        .count();
    let top: usize = all
        .items
        .iter()
        .filter(|m| m.depth == 1)
        .map(|m| m.files)
        .sum();
    assert_eq!(find(&all.items, ".").files, direct + top);
    // Everything serialises without nulls.
    let v = serde_json::to_value(&all).unwrap();
    assert!(v["items"][0]["path"] == ".");
}
