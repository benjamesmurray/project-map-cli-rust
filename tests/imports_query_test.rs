//! Task 3.5: `imports_result`, the files that import an external package.

mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::{ErrorCode, ImportsResult};
use std::path::Path;
use tempfile::{tempdir, TempDir};

fn build(root: &Path, base: &Path) -> std::path::PathBuf {
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &base.join("out")).unwrap();
    let index = base.join("index.json");
    orch.save_index(&index).unwrap();
    index
}

fn engine_for(fixture: &str) -> (TempDir, QueryEngine) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(fixture, &root);
    let index = build(&root, dir.path());
    (dir, QueryEngine::load(&index).unwrap())
}

fn engine_with(files: &[(&str, &str)]) -> (TempDir, QueryEngine) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    for (path, text) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    let index = build(&root, dir.path());
    (dir, QueryEngine::load(&index).unwrap())
}

fn places(r: &ImportsResult) -> Vec<(&str, usize)> {
    r.items.iter().map(|i| (i.path.as_str(), i.line)).collect()
}

#[test]
fn scoped_packages_match_by_prefix_across_languages() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let r = engine.imports_result("@sentry/*", None, None).unwrap();
    assert_eq!(r.query, "@sentry/*");
    assert_eq!(
        places(&r),
        [
            ("js/app.js", 1),
            ("js/lazy.mjs", 1),
            ("js/legacy.cjs", 2),
            ("js/view.jsx", 2),
            ("ts/client.ts", 1)
        ]
    );
    assert_eq!((r.files, r.imports, r.truncated), (5, 5, false));
    let tracing = r.items.iter().find(|i| i.path == "js/legacy.cjs").unwrap();
    assert_eq!(tracing.module, "@sentry/node/tracing");
    assert_eq!(tracing.package.as_deref(), Some("@sentry/node"));
    assert_eq!(tracing.language, "typescript");
    assert!(!tracing.resolved);
    assert!(
        tracing.text.contains("@sentry/node/tracing"),
        "{}",
        tracing.text
    );

    // An exact scoped name matches only that package.
    let node = engine.imports_result("@sentry/node", None, None).unwrap();
    assert_eq!(places(&node), [("js/app.js", 1), ("js/legacy.cjs", 2)]);
}

#[test]
fn an_exact_package_lists_its_import_lines() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let stripe = engine.imports_result("stripe", None, None).unwrap();
    assert_eq!(
        places(&stripe),
        [("js/app.js", 2), ("js/legacy.cjs", 1), ("ts/client.ts", 4)]
    );
    // The bundle that is skipped as build output is not reported.
    assert!(stripe.items.iter().all(|i| i.path != "js/bundle.js"));

    let sentry = engine.imports_result("sentry_sdk", None, None).unwrap();
    assert_eq!(
        sentry
            .items
            .iter()
            .map(|i| (i.line, i.module.as_str()))
            .collect::<Vec<_>>(),
        [
            (1, "sentry_sdk"),
            (2, "sentry_sdk.integrations"),
            (3, "sentry_sdk.integrations.django")
        ]
    );
    assert!(sentry.items.iter().all(|i| i.path == "python/app.py"));
    assert_eq!((sentry.files, sentry.imports), (1, 3));

    let serde = engine.imports_result("serde_json", None, None).unwrap();
    assert_eq!(
        places(&serde),
        [
            ("rust_crate/src/lib.rs", 1),
            ("rust_crate/src/lib.rs", 8),
            ("rust_crate/tests/uses_crate_name.rs", 2)
        ]
    );
    assert_eq!((serde.files, serde.imports), (2, 3));
    assert!(serde.items.iter().all(|i| i.language == "rust"));

    // `std` is a package like any other crate.
    assert_eq!(
        places(&engine.imports_result("std", None, None).unwrap()),
        [("rust_crate/src/user.rs", 3)]
    );
    // A PHP namespace.
    assert_eq!(
        places(&engine.imports_result("Stripe", None, None).unwrap()),
        [("php/Billing.php", 5)]
    );
}

#[test]
fn relative_and_crate_imports_never_match() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    for q in [
        "./util",
        "../ts/shared",
        "./target",
        "crate",
        "self",
        "super",
        "fixture_crate",
        ".",
        "",
        "  ",
        "*",
        "/*",
        ".*",
    ] {
        match engine.imports_result(q, None, None) {
            Ok(r) => assert!(r.items.is_empty() && r.imports == 0, "{q}: {r:?}"),
            Err(e) => assert_eq!(e.code, ErrorCode::InvalidArgument, "{q}"),
        }
    }
    // An unknown package is an empty result, not an error.
    let none = engine.imports_result("left-pad", None, None).unwrap();
    assert_eq!((none.files, none.imports, none.truncated), (0, 0, false));
    assert!(none.items.is_empty());
}

#[test]
fn an_empty_package_is_an_invalid_argument() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    for q in ["", "   ", "/*", ".*"] {
        let err = engine.imports_result(q, None, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument, "{q:?}");
    }
}

#[test]
fn limit_truncates_the_list_but_not_the_counts() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let all = engine.imports_result("@sentry/*", None, None).unwrap();
    let two = engine.imports_result("@sentry/*", None, Some(2)).unwrap();
    assert!(two.truncated);
    assert_eq!(two.items, all.items[..2]);
    assert_eq!((two.files, two.imports), (all.files, all.imports));
    let zero = engine.imports_result("@sentry/*", None, Some(0)).unwrap();
    assert!(zero.items.is_empty() && zero.truncated && zero.imports == 5);
    let exact = engine.imports_result("@sentry/*", None, Some(5)).unwrap();
    assert!(!exact.truncated);

    // The default limit is 50.
    let lines: String = (0..60)
        .map(|i| format!("import m{i}\nimport requests\n"))
        .collect();
    let (_d, big) = engine_with(&[("many.py", &lines)]);
    let r = big.imports_result("requests", None, None).unwrap();
    assert_eq!(
        (r.items.len(), r.imports, r.files, r.truncated),
        (50, 60, 1, true)
    );
}

#[test]
fn path_prefix_narrows_the_files() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let js = engine
        .imports_result("@sentry/*", Some("js"), None)
        .unwrap();
    assert_eq!(js.imports, 4);
    assert!(js.items.iter().all(|i| i.path.starts_with("js/")));
    for spelled in ["js/", "./js", "./js/"] {
        let r = engine
            .imports_result("@sentry/*", Some(spelled), None)
            .unwrap();
        assert_eq!(r.items, js.items, "{spelled}");
    }
    // A single file works as a prefix; a name that only starts the same does not match.
    let one = engine
        .imports_result("@sentry/*", Some("ts/client.ts"), None)
        .unwrap();
    assert_eq!(places(&one), [("ts/client.ts", 1)]);
    let partial = engine.imports_result("@sentry/*", Some("j"), None).unwrap();
    assert_eq!(partial.imports, 0);
}

#[test]
fn imports_on_one_line_are_ordered_by_module() {
    let (_dir, engine) = engine_with(&[(
        "multi.py",
        "import sentry_sdk.integrations, sentry_sdk, sentry_sdk.api\n",
    )]);
    let r = engine.imports_result("sentry_sdk", None, None).unwrap();
    let modules: Vec<_> = r.items.iter().map(|i| i.module.as_str()).collect();
    assert_eq!(
        modules,
        ["sentry_sdk", "sentry_sdk.api", "sentry_sdk.integrations"]
    );
    assert!(r.items.iter().all(|i| i.line == 1));
    assert_eq!((r.files, r.imports), (1, 3));
}

#[test]
fn resolved_marks_imports_linked_to_an_indexed_file() {
    let (_dir, engine) = engine_with(&[
        ("util.py", "def helper():\n    pass\n"),
        ("main.py", "import util\nimport requests\n"),
    ]);
    // `util` has a package name (Python takes the first segment) and resolves to util.py.
    let util = engine.imports_result("util", None, None).unwrap();
    assert_eq!(util.items.len(), 1);
    assert!(util.items[0].resolved);
    let requests = engine.imports_result("requests", None, None).unwrap();
    assert!(!requests.items[0].resolved);
}

#[test]
fn a_schema_3_index_is_index_stale() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(IMPORTS_FIXTURE, &root);
    let index = build(&root, dir.path());
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    json["meta"]["schema_version"] = 3.into();
    json.as_object_mut().unwrap().remove("imports");
    std::fs::write(&index, json.to_string()).unwrap();

    let old = QueryEngine::load(&index).unwrap();
    assert!(old.is_stale());
    let err = old.imports_result("stripe", None, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::IndexStale);
    // Code tools keep working on it.
    assert!(!old.find_symbols("target").is_empty());
}

#[test]
fn results_serialise_without_nulls() {
    let (_dir, engine) = engine_for(IMPORTS_FIXTURE);
    let v = serde_json::to_value(engine.imports_result("@sentry/*", None, None).unwrap()).unwrap();
    assert_eq!(v["imports"], 5);
    let item = &v["items"][0];
    for field in [
        "path", "line", "text", "module", "package", "language", "resolved",
    ] {
        assert!(item.get(field).is_some(), "{field}");
    }
}
