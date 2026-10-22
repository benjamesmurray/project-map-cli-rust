//! Task 2.3: JavaScript indexing, the new import captures and the skip rules for built JS.

mod common;

use common::{copy_fixture, write_large_js, IMPORTS_FIXTURE};
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::parser::CodeParser;
use project_map_cli_rust::core::query_engine::QueryEngine;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// The parsed file, with `imports` reduced to the module strings.
struct Parsed {
    language: String,
    symbols: Vec<project_map_cli_rust::core::parser::Symbol>,
    imports: Vec<String>,
}

fn modules(outline: project_map_cli_rust::core::parser::FileOutline) -> Parsed {
    Parsed {
        language: outline.language,
        symbols: outline.symbols,
        imports: outline.imports.into_iter().map(|r| r.module).collect(),
    }
}

fn parse(rel: &str) -> Parsed {
    modules(
        CodeParser::new()
            .parse_file(&Path::new(IMPORTS_FIXTURE).join(rel))
            .unwrap_or_else(|e| panic!("{rel}: {e}")),
    )
}

fn build(root: &Path) -> QueryEngine {
    let out = root.parent().unwrap().join("out");
    let mut orch = Orchestrator::new();
    orch.build_index(root, &out).expect("build");
    let index = out.join("index.json");
    orch.save_index(&index).expect("save");
    QueryEngine::load(&index).expect("load")
}

#[test]
fn hand_written_javascript_symbols_are_indexed() {
    let app = parse("js/app.js");
    assert_eq!(app.language, "typescript");
    let names: Vec<&str> = app.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Checkout"), "{names:?}");
    assert!(names.contains(&"start"), "{names:?}");
    assert!(names.contains(&"total"), "{names:?}");
    for rel in ["js/lazy.mjs", "js/legacy.cjs", "js/view.jsx", "js/util.js"] {
        assert!(!parse(rel).symbols.is_empty(), "{rel} has symbols");
    }
}

#[test]
fn javascript_fixtures_parse_without_error_nodes() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .unwrap();
    for rel in [
        "js/app.js",
        "js/lazy.mjs",
        "js/legacy.cjs",
        "js/view.jsx",
        "js/util.js",
    ] {
        let src = fs::read_to_string(Path::new(IMPORTS_FIXTURE).join(rel)).unwrap();
        let tree = parser.parse(&src, None).unwrap();
        assert!(!tree.root_node().has_error(), "{rel} has ERROR nodes");
    }
}

#[test]
fn es_import_require_dynamic_import_and_reexport_are_captured() {
    assert_eq!(
        parse("js/app.js").imports,
        [
            "@sentry/node",
            "stripe",
            "posthog-js",
            "./util.js",
            "../ts/shared"
        ]
    );
    assert_eq!(
        parse("js/legacy.cjs").imports,
        ["stripe", "@sentry/node/tracing", "./util.js"]
    );
    assert_eq!(
        parse("js/lazy.mjs").imports,
        ["@sentry/react", "posthog-js"],
        "export-from and dynamic import()"
    );
    assert_eq!(
        parse("js/view.jsx").imports,
        ["react", "@sentry/react", "./util"]
    );
    assert_eq!(
        parse("ts/client.ts").imports,
        ["@sentry/react", "./shared", "stripe", "posthog-js"],
        "require() and import() in TypeScript"
    );
}

#[test]
fn require_of_a_non_literal_or_other_function_is_not_an_import() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("x.js");
    fs::write(
        &file,
        "const a = require(name);\nconst b = load(\"nope\");\nconst c = import(path);\nconst d = require(\"real\");\n",
    )
    .unwrap();
    assert_eq!(
        modules(CodeParser::new().parse_file(&file).unwrap()).imports,
        ["real"]
    );
}

#[test]
fn rust_use_and_extern_crate_are_captured_one_record_per_statement() {
    assert_eq!(
        parse("rust_crate/src/lib.rs").imports,
        ["serde_json", "serde_json::Value"]
    );
    assert_eq!(
        parse("rust_crate/src/user.rs").imports,
        [
            "crate::a::b::Sym",
            "crate::a::{b, c}",
            "std::collections::HashMap"
        ],
        "a grouped use is one record"
    );
    assert_eq!(
        parse("rust_crate/src/a/mod.rs").imports,
        ["self::b::Sym", "super::c::Gamma"]
    );
}

#[test]
fn multi_line_grouped_use_is_one_record_with_collapsed_whitespace() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("m.rs");
    fs::write(&file, "use std::{\n    fs,\n    io::Read,\n};\n").unwrap();
    assert_eq!(
        modules(CodeParser::new().parse_file(&file).unwrap()).imports,
        ["std::{ fs, io::Read, }"]
    );
}

#[test]
fn minified_bundled_and_large_javascript_are_not_indexed() {
    let temp = tempdir().unwrap();
    let root = copy_fixture(IMPORTS_FIXTURE, &temp.path().join("project"));
    let large = write_large_js(&root);
    let engine = build(&root);
    let files = engine.get_all_file_paths();

    for skipped in ["js/vendor.min.js", "js/bundle.js", large] {
        assert!(
            !files.iter().any(|p| p.ends_with(skipped)),
            "{skipped} must not be indexed: {files:?}"
        );
        assert!(
            CodeParser::new().parse_file(&root.join(skipped)).is_err(),
            "{skipped}"
        );
    }
    for kept in [
        "js/app.js",
        "js/lazy.mjs",
        "js/legacy.cjs",
        "js/view.jsx",
        "js/util.js",
    ] {
        assert!(
            files.iter().any(|p| p.ends_with(kept)),
            "{kept} must be indexed: {files:?}"
        );
    }
    assert!(!engine.find_symbols("Checkout").is_empty());
}

#[test]
fn each_skip_rule_applies_on_its_own() {
    let temp = tempdir().unwrap();
    let parser = || CodeParser::new();

    let min = temp.path().join("lib.min.js");
    fs::write(&min, "export const a = 1;\n").unwrap();
    assert!(parser().parse_file(&min).is_err(), "*.min.* by name");

    let long = temp.path().join("long.mjs");
    fs::write(
        &long,
        format!("export const a = \"{}\";\n", "x".repeat(1001)),
    )
    .unwrap();
    assert!(parser().parse_file(&long).is_err(), "line over 1,000 chars");

    let edge = temp.path().join("edge.js");
    fs::write(
        &edge,
        format!("// {}\nexport const a = 1;\n", "x".repeat(990)),
    )
    .unwrap();
    assert!(parser().parse_file(&edge).is_ok(), "line under the limit");

    let late = temp.path().join("late.js");
    let mut src = "export const a = 1;\n".repeat(2000);
    src.push_str(&format!("// {}\n", "y".repeat(2000)));
    fs::write(&late, src).unwrap();
    assert!(
        parser().parse_file(&late).is_ok(),
        "a long line after the first 32 KB is not examined"
    );

    let ts = temp.path().join("long.ts");
    fs::write(&ts, format!("export const a = \"{}\";\n", "x".repeat(5000))).unwrap();
    assert!(parser().parse_file(&ts).is_ok(), "TypeScript is unaffected");
}
