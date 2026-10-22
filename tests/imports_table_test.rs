//! Task 2.5: every import is kept in the imports side table, with `resolved` and `package`,
//! and Rust `use` paths resolve to `Imports` edges.

mod common;

use common::{copy_fixture, IMPORTS_FIXTURE};
use petgraph::visit::EdgeRef;
use project_map_cli_rust::core::graph::{EdgeType, ImportEntry, NodeType, ProjectGraph};
use project_map_cli_rust::core::orchestrator::Orchestrator;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::tempdir;

fn build(root: &Path) -> ProjectGraph {
    let out = root.parent().unwrap().join("out");
    let mut orch = Orchestrator::new();
    orch.build_index(root, &out).expect("build");
    let index = out.join("index.json");
    orch.save_index(&index).expect("save");
    ProjectGraph::load(&index).expect("load")
}

fn fixture_graph() -> (tempfile::TempDir, ProjectGraph) {
    let temp = tempdir().unwrap();
    let root = copy_fixture(IMPORTS_FIXTURE, &temp.path().join("project"));
    let graph = build(&root);
    (temp, graph)
}

fn entry<'a>(g: &'a ProjectGraph, path: &str, module: &str) -> &'a ImportEntry {
    g.imports
        .iter()
        .find(|e| e.path == path && e.module == module)
        .unwrap_or_else(|| panic!("no import {module} in {path}"))
}

/// Imports edges as (importer path, target "path" or "path::Symbol").
fn import_edges(g: &ProjectGraph) -> Vec<(String, String)> {
    let mut edges: Vec<(String, String)> = g
        .graph
        .edge_references()
        .filter(|e| *e.weight() == EdgeType::Imports)
        .map(|e| {
            let from = &g.graph[e.source()];
            let to = &g.graph[e.target()];
            let target = if to.node_type == NodeType::File {
                to.path.clone()
            } else {
                format!("{}::{}", to.path, to.name)
            };
            (from.path.clone(), target)
        })
        .collect();
    edges.sort();
    edges
}

fn rust_edges(g: &ProjectGraph) -> Vec<(String, String)> {
    import_edges(g)
        .into_iter()
        .filter(|(from, _)| from.starts_with("rust_crate/"))
        .collect()
}

#[test]
fn relative_import_of_an_indexed_file_is_resolved_without_a_package() {
    let (_t, g) = fixture_graph();
    let e = entry(&g, "js/app.js", "./util.js");
    assert!(e.resolved, "{e:?}");
    assert_eq!(e.package, None);
    assert_eq!(e.line, 4);
    assert_eq!(e.text, "import { formatAmount } from \"./util.js\";");
    assert_eq!(e.language, "typescript");
    assert!(import_edges(&g).contains(&("js/app.js".into(), "js/util.js".into())));

    let e = entry(&g, "js/app.js", "../ts/shared");
    assert!(e.resolved && e.package.is_none());
    assert!(import_edges(&g).contains(&("js/app.js".into(), "ts/shared.ts".into())));
}

#[test]
fn external_package_is_unresolved_and_named() {
    let (_t, g) = fixture_graph();
    let e = entry(&g, "js/app.js", "@sentry/node");
    assert!(!e.resolved);
    assert_eq!(e.package.as_deref(), Some("@sentry/node"));
    assert_eq!(e.line, 1);

    let e = entry(&g, "js/legacy.cjs", "@sentry/node/tracing");
    assert_eq!(e.package.as_deref(), Some("@sentry/node"));
    assert_eq!(
        entry(&g, "js/legacy.cjs", "stripe").package.as_deref(),
        Some("stripe")
    );
    assert_eq!(
        entry(&g, "ts/client.ts", "posthog-js").package.as_deref(),
        Some("posthog-js")
    );
    assert_eq!(
        entry(&g, "php/Billing.php", "Stripe.StripeClient")
            .package
            .as_deref(),
        Some("Stripe")
    );
}

#[test]
fn python_imports_on_one_line_are_two_records() {
    let (_t, g) = fixture_graph();
    let line1: Vec<&ImportEntry> = g
        .imports
        .iter()
        .filter(|e| e.path == "python/app.py" && e.line == 1)
        .collect();
    assert_eq!(line1.len(), 2, "{line1:?}");
    assert_eq!(line1[0].module, "json");
    assert_eq!(line1[1].module, "sentry_sdk");
    assert_eq!(line1[1].package.as_deref(), Some("sentry_sdk"));
    assert_eq!(line1[0].text, line1[1].text);
}

#[test]
fn side_table_is_sorted_by_path_then_line_then_module() {
    let (_t, g) = fixture_graph();
    assert!(!g.imports.is_empty());
    let keys: Vec<_> = g
        .imports
        .iter()
        .map(|e| (e.path.clone(), e.line, e.module.clone()))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn skipped_javascript_has_no_import_records() {
    let (_t, g) = fixture_graph();
    assert!(!g.imports.iter().any(|e| e.path == "js/bundle.js"));
    assert!(!g.imports.iter().any(|e| e.path == "js/vendor.min.js"));
}

#[test]
fn rust_use_forms_resolve_to_imports_edges() {
    let (_t, g) = fixture_graph();
    let edges = rust_edges(&g);
    let has = |from: &str, to: &str| edges.contains(&(from.to_string(), to.to_string()));

    // crate:: to a symbol
    assert!(
        has("rust_crate/src/user.rs", "rust_crate/src/a/b.rs::Sym"),
        "{edges:?}"
    );
    // grouped use: each leaf is a module file
    assert!(
        has("rust_crate/src/user.rs", "rust_crate/src/a/b.rs"),
        "{edges:?}"
    );
    assert!(
        has("rust_crate/src/user.rs", "rust_crate/src/a/c.rs"),
        "{edges:?}"
    );
    // self:: and super::
    assert!(
        has("rust_crate/src/a/mod.rs", "rust_crate/src/a/b.rs::Sym"),
        "{edges:?}"
    );
    assert!(
        has("rust_crate/src/a/mod.rs", "rust_crate/src/c/mod.rs::Gamma"),
        "{edges:?}"
    );
    // crate:: to a mod.rs module
    assert!(
        has("rust_crate/src/a/b.rs", "rust_crate/src/c/mod.rs::Gamma"),
        "{edges:?}"
    );
    // the crate's own name, from an integration test
    assert!(
        has(
            "rust_crate/tests/uses_crate_name.rs",
            "rust_crate/src/a/b.rs::Sym"
        ),
        "{edges:?}"
    );
    assert_eq!(edges.len(), 7, "no other Rust edges: {edges:?}");
}

#[test]
fn rust_records_carry_resolved_and_package() {
    let (_t, g) = fixture_graph();
    let user = |m: &str| entry(&g, "rust_crate/src/user.rs", m);
    assert!(user("crate::a::b::Sym").resolved);
    assert_eq!(user("crate::a::b::Sym").package, None);
    assert!(
        user("crate::a::{b, c}").resolved,
        "grouped use is one record"
    );
    assert_eq!(user("crate::a::{b, c}").line, 2);

    let std = user("std::collections::HashMap");
    assert!(!std.resolved);
    assert_eq!(std.package.as_deref(), Some("std"));

    let lib = |m: &str| entry(&g, "rust_crate/src/lib.rs", m);
    assert!(!lib("serde_json").resolved);
    assert_eq!(lib("serde_json").package.as_deref(), Some("serde_json"));
    assert_eq!(
        lib("serde_json::Value").package.as_deref(),
        Some("serde_json")
    );

    let own = entry(
        &g,
        "rust_crate/tests/uses_crate_name.rs",
        "fixture_crate::a::b::Sym",
    );
    assert!(own.resolved);
    assert_eq!(own.package, None, "the crate's own name is not a package");
    let ext = entry(
        &g,
        "rust_crate/tests/uses_crate_name.rs",
        "serde_json::Value",
    );
    assert!(!ext.resolved);
    assert_eq!(ext.package.as_deref(), Some("serde_json"));

    assert_eq!(
        entry(&g, "rust_crate/src/a/mod.rs", "self::b::Sym").package,
        None
    );
    assert_eq!(
        entry(&g, "rust_crate/src/a/mod.rs", "super::c::Gamma").package,
        None
    );
}

/// Edge counts by type for each existing non-Rust fixture, from the 0.3.0 binary (task 2.5).
#[test]
fn edge_counts_for_existing_non_rust_fixtures_are_unchanged() {
    let expected: [(&str, &[(&str, usize)]); 13] = [
        ("compatibility", &[]),
        ("docs", &[("Contains", 60), ("References", 17)]),
        ("gdscript", &[("Contains", 3), ("Imports", 2)]),
        ("kotlin", &[("Contains", 4), ("Imports", 1)]),
        ("lua", &[("Contains", 4), ("Imports", 1)]),
        (
            "php_symfony",
            &[("Contains", 10), ("References", 1), ("Imports", 1)],
        ),
        ("python", &[("Contains", 7), ("Imports", 3)]),
        ("rust", &[("Contains", 5)]),
        ("sql", &[("Contains", 3)]),
        (
            "streaming",
            &[
                ("Contains", 10),
                ("References", 3),
                ("Produces", 4),
                ("Configures", 8),
                ("Consumes", 1),
            ],
        ),
        ("terraform", &[("Contains", 5), ("Imports", 1)]),
        ("typescript", &[("Contains", 4), ("Imports", 2)]),
        ("vue", &[("Contains", 15)]),
    ];
    for (name, counts) in expected {
        // Built in place: some fixtures name their own repository path (gdscript `res://`).
        let temp = tempdir().unwrap();
        let mut orch = Orchestrator::new();
        orch.build_index(Path::new(&format!("tests/fixtures/{name}")), temp.path())
            .expect("build");
        let index = temp.path().join("index.json");
        orch.save_index(&index).expect("save");
        let g = ProjectGraph::load(&index).expect("load");
        let mut got: BTreeMap<String, usize> = BTreeMap::new();
        for e in g.graph.edge_references() {
            *got.entry(format!("{:?}", e.weight())).or_default() += 1;
        }
        let want: BTreeMap<String, usize> =
            counts.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        assert_eq!(got, want, "fixture {name}");
    }
}
