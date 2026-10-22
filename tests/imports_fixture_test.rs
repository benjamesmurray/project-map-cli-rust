mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn fixture_tree_has_every_documented_file() {
    let readme = std::fs::read_to_string(Path::new(IMPORTS_FIXTURE).join("README.md")).unwrap();
    for line in readme.lines().filter(|l| l.starts_with("| `")) {
        let cell = line.trim_start_matches("| `");
        let path = cell.split('`').next().unwrap();
        if path.contains('…') {
            continue;
        }
        assert!(
            Path::new(IMPORTS_FIXTURE).join(path).exists(),
            "README lists {path}, which does not exist"
        );
    }
}

#[test]
fn git_fixture_makes_one_deterministic_commit() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let sha_a = git_fixture(IMPORTS_FIXTURE, &a.path().join("one"));
    let sha_b = git_fixture(IMPORTS_FIXTURE, &b.path().join("two"));
    assert_eq!(sha_a.len(), 40);
    assert!(sha_a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(sha_a, sha_b, "a fixed author, date and tree give one SHA");
    assert_eq!(git(&a.path().join("one"), &["rev-parse", "HEAD"]), sha_a);
    assert_eq!(git(&a.path().join("one"), &["status", "--porcelain"]), "");
}

#[test]
fn large_js_helper_writes_a_file_over_the_size_limit() {
    let dir = tempdir().unwrap();
    copy_fixture(IMPORTS_FIXTURE, dir.path());
    let rel = write_large_js(dir.path());
    assert!(std::fs::metadata(dir.path().join(rel)).unwrap().len() > 256 * 1024);
}

#[test]
fn skipped_js_fixtures_are_what_the_readme_says() {
    let long = std::fs::read_to_string(Path::new(IMPORTS_FIXTURE).join("js/bundle.js")).unwrap();
    let head: String = long.chars().take(32 * 1024).collect();
    assert!(head.lines().any(|l| l.len() > 1000));
    assert!(Path::new(IMPORTS_FIXTURE).join("js/vendor.min.js").exists());
}

#[test]
fn fixture_tree_builds() {
    let out = tempdir().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new(IMPORTS_FIXTURE), out.path())
        .unwrap();
}

#[test]
fn qualified_names_on_the_parsed_fixture_are_not_unique() {
    use project_map_cli_rust::core::graph::{NodeType, ProjectGraph};
    use project_map_cli_rust::core::results::QualifiedNamer;

    let out = tempdir().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new(IMPORTS_FIXTURE), out.path())
        .unwrap();
    let file = out.path().join("index.json");
    orch.save_index(&file).unwrap();
    let g = ProjectGraph::load(&file).unwrap();

    let nodes: Vec<_> = g.graph.node_weights().collect();
    let namer = QualifiedNamer::new(nodes.iter().copied());
    let in_errors = |name: &str| -> Vec<_> {
        nodes
            .iter()
            .filter(|n| {
                n.path.ends_with("rust_crate/src/errors.rs")
                    && n.name == name
                    && n.node_type == NodeType::Symbol
            })
            .collect()
    };
    let froms = in_errors("from");
    assert_eq!(froms.len(), 2, "two From impls give two `from` methods");
    let names: Vec<_> = froms.iter().map(|n| namer.qualify(n)).collect();
    assert_eq!(names, ["E.from", "E.from"]);
    assert_ne!(
        froms[0].line, froms[1].line,
        "a citation needs the line as well"
    );
}
