//! Suffix-match import resolution (Strategy 3) and the determinism it needs: the same tree
//! always gives the same graph, the same import table and the same index bytes.

mod common;

use common::copy_fixture;
use petgraph::visit::EdgeRef;
use project_map_cli_rust::core::graph::{EdgeType, ProjectGraph};
use project_map_cli_rust::core::orchestrator::Orchestrator;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const AMBIGUOUS: &str = "tests/fixtures/ambiguous_imports";

/// Builds `root` (absolute) and returns the saved index path.
fn build(root: &Path, base: &Path) -> std::path::PathBuf {
    let mut orch = Orchestrator::new();
    orch.build_index(root, &base.join("out")).expect("build");
    let index = base.join("index.json");
    orch.save_index(&index).expect("save");
    index
}

fn graph_of(root: &Path, base: &Path) -> ProjectGraph {
    ProjectGraph::load(&build(root, base)).expect("load")
}

/// Every edge as (from path, from symbol, to path, to symbol, type), sorted.
fn edge_tuples(g: &ProjectGraph) -> Vec<(String, String, String, String, String)> {
    let mut edges: Vec<_> = g
        .graph
        .edge_references()
        .map(|e| {
            let (a, b) = (&g.graph[e.source()], &g.graph[e.target()]);
            (
                a.path.clone(),
                a.name.clone(),
                b.path.clone(),
                b.name.clone(),
                format!("{:?}", e.weight()),
            )
        })
        .collect();
    edges.sort();
    edges
}

/// File-to-file `Imports` edges as (importer path, target path), sorted.
fn import_edges(g: &ProjectGraph) -> Vec<(String, String)> {
    let mut edges: Vec<_> = g
        .graph
        .edge_references()
        .filter(|e| *e.weight() == EdgeType::Imports)
        .map(|e| {
            (
                g.graph[e.source()].path.clone(),
                g.graph[e.target()].path.clone(),
            )
        })
        .collect();
    edges.sort();
    edges
}

fn fixture_in(dir: &Path, name: &str) -> std::path::PathBuf {
    copy_fixture(AMBIGUOUS, &dir.join(name))
        .canonicalize()
        .unwrap()
}

#[test]
fn fixture_edges_match_the_readme() {
    let temp = tempdir().unwrap();
    let g = graph_of(&fixture_in(temp.path(), "project"), temp.path());
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        import_edges(&g),
        vec![
            pair("src/app/service.py", "src/app/config.py"),
            pair("src/app/tier2.py", "pkg/mod.py"),
            pair("src/app/tier3.py", "vendor/lib3/mod3.py"),
        ]
    );
}

#[test]
fn tied_and_unmatched_imports_stay_unresolved_and_the_rest_are_resolved() {
    let temp = tempdir().unwrap();
    let g = graph_of(&fixture_in(temp.path(), "project"), temp.path());
    let resolved = |path: &str, module: &str| {
        g.imports
            .iter()
            .find(|e| e.path == path && e.module == module)
            .unwrap_or_else(|| panic!("no import {module} in {path}"))
            .resolved
    };
    assert!(resolved("src/app/service.py", "app.config"));
    assert!(resolved("src/app/tier2.py", "pkg.mod.Thing"));
    assert!(resolved("src/app/tier3.py", "ext.lib3.mod3"));
    assert!(
        !resolved("src/app/tie.py", "shared.util"),
        "a tie is not linked"
    );
    assert!(
        !resolved("src/app/cache.py", "redis"),
        "redis is not myredis"
    );
}

#[test]
fn the_farther_candidate_is_never_linked() {
    let temp = tempdir().unwrap();
    let g = graph_of(&fixture_in(temp.path(), "project"), temp.path());
    for (_, to) in import_edges(&g) {
        assert!(!to.starts_with("tests/fixtures_copy/"), "{to}");
        assert!(!to.starts_with("lib_"), "{to}");
        assert_ne!(to, "src/myredis.py");
    }
}

#[test]
fn the_ambiguous_import_is_counted_in_the_debug_log() {
    let temp = tempdir().unwrap();
    let root = fixture_in(temp.path(), "project");
    let out = Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
        .args(["build", "-r", ".", "-o", ".project-map"])
        .env("PROJECT_MAP_LOG", "debug")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(out.status.success());
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(
        log.contains("left unlinked by an ambiguous suffix match: 1"),
        "{log}"
    );
    assert!(
        log.contains("lib_a.shared.util, lib_b.shared.util"),
        "candidates are listed, sorted: {log}"
    );
}

#[test]
fn twenty_builds_in_one_process_give_the_same_graph_and_import_table() {
    let temp = tempdir().unwrap();
    let root = fixture_in(temp.path(), "project");
    let first = graph_of(&root, &temp.path().join("b0"));
    let (edges, imports) = (edge_tuples(&first), first.imports.clone());
    assert!(!edges.is_empty());
    for i in 1..20 {
        let g = graph_of(&root, &temp.path().join(format!("b{i}")));
        assert_eq!(edge_tuples(&g), edges, "build {i}: edges differ");
        assert_eq!(g.imports, imports, "build {i}: import table differs");
    }
}

/// The index file with the fields that name the directory or the build removed.
fn index_without_build_fields(path: &Path) -> String {
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let meta = v["meta"].as_object_mut().unwrap();
    for key in [
        "built_at",
        "builder_pid",
        "root",
        "git_commit",
        "git_dirty",
        "git_branch",
    ] {
        meta.remove(key);
    }
    v.to_string()
}

#[test]
fn two_directories_give_a_byte_identical_index() {
    for fixture in [AMBIGUOUS, "tests/fixtures/imports", "tests/fixtures/docs"] {
        let temp = tempdir().unwrap();
        let one = copy_fixture(fixture, &temp.path().join("alpha"));
        let two = copy_fixture(fixture, &temp.path().join("omega-with-a-much-longer-name"));
        let a = build(&one.canonicalize().unwrap(), &temp.path().join("a"));
        let b = build(&two.canonicalize().unwrap(), &temp.path().join("b"));
        assert_eq!(
            index_without_build_fields(&a),
            index_without_build_fields(&b),
            "{fixture}: index differs between directories"
        );
    }
}

#[test]
fn blast_summary_lists_equal_sized_directories_in_name_order() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("target.py"), "def run():\n    pass\n").unwrap();
    for dir in ["h", "c", "a", "g", "e", "b", "f", "d"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
        std::fs::write(root.join(dir).join("user.py"), "import target\n").unwrap();
    }
    let cli = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    };
    cli(&["build", "-r", ".", "-o", ".project-map"]);
    for _ in 0..10 {
        let text = cli(&["blast", "-p", "target.py", "-s", "target"]);
        let dirs: Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("* "))
            .filter_map(|l| l.split(':').next())
            .collect();
        assert_eq!(&dirs[..5], ["a", "b", "c", "d", "e"], "{text}");
    }
}
