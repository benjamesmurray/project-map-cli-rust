//! Task 3.6: every list has a total order and target selection is deterministic, so two
//! builds of one tree from differently named directories give byte-identical output.

mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::BlastMatch;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use tempfile::{tempdir, TempDir};

const DOCS: &str = "tests/fixtures/docs";
const AMBIGUOUS: &str = "tests/fixtures/ambiguous_imports";

const BIN: &str = env!("CARGO_BIN_EXE_project-map-cli-rust");

/// Runs the CLI in `dir` and returns its stdout.
fn cli(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `status` minus the lines that name the directory or the build.
fn status_without_build_fields(text: &str) -> String {
    text.lines()
        .filter(|l| {
            !["Root:", "Built At:", "Index:", "Commit:"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn calls(fixture: &str) -> Vec<Vec<&'static str>> {
    let mut all: Vec<Vec<&'static str>> = vec![vec!["status"]];
    if fixture == DOCS {
        all.extend([
            vec!["find", "-q", "append"],
            vec!["find", "-q", "audit"],
            vec!["find", "-q", "flush"],
            vec!["find", "-f", "src"],
            vec!["search", "retention"],
            vec!["context", "-p", "src/audit/log.rs"],
            vec!["context", "-p", "src/billing/invoice.py"],
            vec!["blast", "-p", "src/audit/log.rs", "-s", "append"],
            vec!["blast", "-p", "src/billing/invoice.py", "-s", "apply"],
            vec!["blast", "-p", "src/audit/retention.rs", "-s", "nope"],
            vec!["impact", "-f", "flush_buffer"],
            vec!["impact", "-f", "RetentionPolicy"],
            vec!["impact", "-f", "apply"],
            vec!["fetch", "-p", "src/billing/invoice.py", "-s", "apply"],
            vec!["fetch", "-p", "src/audit/retention.rs", "-s", "new"],
            vec!["doc", "search", "audit log"],
            vec!["doc", "outline", "docs/guide.md"],
            vec!["doc", "section", "docs/guide.md", "usage"],
            vec!["doc", "links", "src/audit/log.rs"],
            vec!["doc", "links", "docs/links.md"],
            vec!["doc", "terms"],
            vec!["api"],
            vec!["modules", "--max-depth", "3"],
            vec!["modules", "src", "--max-depth", "1"],
            vec!["imports", "serde"],
        ]);
    } else if fixture == AMBIGUOUS {
        all.extend([
            vec!["find", "-q", "make"],
            vec!["search", "tied"],
            vec!["context", "-p", "src/app/service.py"],
            vec!["blast", "-p", "src/app/config.py", "-s", "SETTING"],
            vec![
                "blast",
                "-p",
                "tests/fixtures_copy/app/config.py",
                "-s",
                "COPY_SETTING",
            ],
            vec!["blast", "-p", "lib_a/shared/util.py", "-s", "VALUE"],
            vec!["blast", "-p", "lib_b/shared/util.py", "-s", "VALUE"],
            vec!["blast", "-p", "pkg/mod.py", "-s", "Thing"],
            vec!["blast", "-p", "src/myredis.py", "-s", "CLIENT"],
            vec!["impact", "-f", "make3"],
            vec!["modules", "--max-depth", "3"],
            vec!["imports", "redis"],
            vec!["imports", "shared"],
            vec!["imports", "pkg"],
        ]);
    } else {
        all.extend([
            vec!["find", "-q", "from"],
            vec!["find", "-q", "Sym"],
            vec!["find", "-f", "rust_crate"],
            vec!["search", "Gamma"],
            vec!["context", "-p", "rust_crate/src/errors.rs"],
            vec!["context", "-p", "rust_crate/src/user.rs"],
            vec!["blast", "-p", "rust_crate/src/errors.rs", "-s", "from"],
            vec!["blast", "-p", "rust_crate/src/c/mod.rs", "-s", "Gamma"],
            vec!["blast", "-p", "diamond/target.ts", "-s", "target"],
            vec!["impact", "-f", "E"],
            vec!["impact", "-f", "from"],
            vec!["impact", "-f", "diamond.a"],
            vec!["fetch", "-p", "rust_crate/src/errors.rs", "-s", "from"],
            vec!["fetch", "-p", "rust_crate/src/errors.rs", "-s", "E"],
            vec!["modules", "--max-depth", "3"],
            vec!["modules", "rust_crate", "--max-depth", "1"],
            vec!["imports", "@sentry/*"],
            vec!["imports", "stripe"],
            vec!["imports", "serde_json"],
            vec!["imports", "sentry_sdk"],
        ]);
    }
    all
}

/// Every call, in TOON and then in JSON.
fn calls_in_both_formats(fixture: &str) -> Vec<Vec<&'static str>> {
    calls(fixture)
        .into_iter()
        .flat_map(|toon| {
            let mut json = toon.clone();
            json.extend(["--format", "json"]);
            [toon, json]
        })
        .collect()
}

/// `status` JSON minus the fields that name the directory, the build or the Git state.
fn status_json_without_build_fields(text: &str) -> String {
    let mut v: Value = serde_json::from_str(text).unwrap();
    for key in [
        "root",
        "built_at",
        "commit",
        "dirty",
        "branch",
        "head_now",
        "dirty_now",
        "at_head",
        "git_error",
    ] {
        v["summary"].as_object_mut().unwrap().remove(key);
    }
    for key in ["commit", "dirty"] {
        v["index"].as_object_mut().unwrap().remove(key);
    }
    v.to_string()
}

/// The output with the build fields of `status` removed, whatever the format.
fn normalised(args: &[&str], text: &str) -> String {
    if args[0] != "status" {
        text.to_string()
    } else if args.contains(&"json") {
        status_json_without_build_fields(text)
    } else {
        status_without_build_fields(text)
    }
}

/// Copies `fixture` to `<temp>/<name>` and builds it there, twice.
fn built_in(fixture: &str, name: &str) -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let root = temp.path().join(name);
    copy_fixture(fixture, &root);
    cli(&root, &["build", "-r", ".", "-o", ".project-map"]);
    (temp, root)
}

fn assert_tool_output_is_stable(fixture: &str) {
    let (_t1, one) = built_in(fixture, "alpha");
    let (_t2, two) = built_in(fixture, "omega-with-a-much-longer-name");
    for args in calls_in_both_formats(fixture) {
        let first = normalised(&args, &cli(&one, &args));
        let second = normalised(&args, &cli(&two, &args));
        assert_eq!(
            first, second,
            "{args:?} differs between the two directories"
        );
    }
    // A second build of the same directory changes nothing.
    let before: Vec<String> = calls_in_both_formats(fixture)
        .iter()
        .map(|a| normalised(a, &cli(&one, a)))
        .collect();
    cli(&one, &["build", "-r", ".", "-o", ".project-map"]);
    for (args, old) in calls_in_both_formats(fixture).iter().zip(before) {
        assert_eq!(
            old,
            normalised(args, &cli(&one, args)),
            "{args:?} differs after a rebuild"
        );
    }
}

#[test]
fn docs_fixture_output_is_identical_across_directories_and_rebuilds() {
    assert_tool_output_is_stable(DOCS);
}

#[test]
fn imports_fixture_output_is_identical_across_directories_and_rebuilds() {
    assert_tool_output_is_stable(IMPORTS_FIXTURE);
}

#[test]
fn ambiguous_imports_fixture_output_is_identical_across_directories_and_rebuilds() {
    assert_tool_output_is_stable(AMBIGUOUS);
}

// --- Typed results, which are what the JSON renderer serialises ---

fn engine_in(fixture: &str, name: &str) -> (TempDir, QueryEngine) {
    let temp = tempdir().unwrap();
    let root = temp.path().join(name);
    copy_fixture(fixture, &root);
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &temp.path().join("out")).unwrap();
    let index = temp.path().join("index.json");
    orch.save_index(&index).unwrap();
    (temp, QueryEngine::load(&index).unwrap())
}

fn all_results(engine: &QueryEngine, fixture: &str) -> Vec<(String, Value)> {
    let v = |x: &dyn erased::Ser| x.value();
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut add = |name: &str, value: Value| out.push((name.to_string(), value));
    for q in ["append", "flush", "from", "Sym", "Gamma", "audit", "e"] {
        add(
            &format!("search {q}"),
            v(&engine.entity_list(q, Some(1), None)),
        );
    }
    for f in ["src", "rust", "a", ""] {
        add(&format!("files {f}"), v(&engine.files_result(f, None)));
    }
    for p in [
        ".",
        "src/audit/log.rs",
        "rust_crate/src/user.rs",
        "rust_crate/src/errors.rs",
    ] {
        if let Ok(o) = engine.outline_result(p) {
            add(&format!("outline {p}"), v(&o));
        }
    }
    for (path, sym) in [
        ("src/audit/log.rs", "append"),
        ("src/audit/retention.rs", "apply"),
        ("src/audit/retention.rs", "nope"),
        ("diamond/target.ts", "target"),
        ("rust_crate/src/c/mod.rs", "Gamma"),
        ("rust_crate/src/a/b.rs", "Sym"),
    ] {
        add(
            &format!("blast {path} {sym}"),
            v(&engine.blast_result(path, sym, None)),
        );
        if let Ok(f) = engine.fetch_result(path, sym) {
            add(&format!("fetch {path} {sym}"), v(&f));
        }
    }
    for name in [
        "flush_buffer",
        "RetentionPolicy",
        "apply",
        "E",
        "from",
        "new",
    ] {
        add(&format!("plan {name}"), v(&engine.plan_result(name, None)));
    }
    add("modules", v(&engine.modules_result(None, 9).unwrap()));
    for pkg in ["@sentry/*", "stripe", "sentry_sdk", "serde_json", "std"] {
        if let Ok(r) = engine.imports_result(pkg, None, None) {
            add(&format!("imports {pkg}"), v(&r));
        }
    }
    if fixture == DOCS {
        add(
            "doc_search",
            v(&engine.doc_search_result("audit", 20).unwrap()),
        );
        for p in [
            "docs/guide.md",
            "docs/links.md",
            "docs/adr/004-immutable-audit-log.md",
        ] {
            add(
                &format!("doc_outline {p}"),
                v(&engine.doc_outline_result(p, None).unwrap()),
            );
            add(
                &format!("doc_links {p}"),
                v(&engine.doc_links_result(p, None).unwrap()),
            );
        }
        add(
            "doc_links code",
            v(&engine.doc_links_result("src/audit/log.rs", None).unwrap()),
        );
        add("doc_terms", v(&engine.doc_terms_result(None, 50).unwrap()));
        add("api", v(&engine.api_surface_result(None).unwrap()));
    }
    out
}

/// Lets `all_results` take any `Serialize` result through one closure.
mod erased {
    use serde::Serialize;
    use serde_json::Value;
    pub trait Ser {
        fn value(&self) -> Value;
    }
    impl<T: Serialize> Ser for T {
        fn value(&self) -> Value {
            serde_json::to_value(self).unwrap()
        }
    }
}

#[test]
fn typed_results_are_identical_across_directories() {
    for fixture in [DOCS, IMPORTS_FIXTURE] {
        let (_a, one) = engine_in(fixture, "alpha");
        let (_b, two) = engine_in(fixture, "omega-with-a-much-longer-name");
        let (r1, r2) = (all_results(&one, fixture), all_results(&two, fixture));
        assert!(r1.len() > 30, "{} results", r1.len());
        for ((name, a), (_, b)) in r1.iter().zip(&r2) {
            assert_eq!(a.to_string(), b.to_string(), "{fixture}: {name}");
        }
    }
}

#[test]
fn search_and_query_lists_are_ordered_by_path_line_name() {
    let (_d, engine) = engine_in(IMPORTS_FIXTURE, "p");
    for q in ["from", "e", "Sym", "a"] {
        let list = engine.entity_list(q, None, None);
        let keys: Vec<_> = list
            .items
            .iter()
            .map(|i| (i.path.clone(), i.line, i.symbol.clone()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        // Qualified names sort differently from bare ones only after path and line tie.
        assert_eq!(
            keys.iter().map(|k| (&k.0, k.1)).collect::<Vec<_>>(),
            sorted.iter().map(|k| (&k.0, k.1)).collect::<Vec<_>>(),
            "{q}"
        );
    }
    let files = engine.files_result("", None).paths;
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
    // The outline of the whole tree is by path as well.
    let tree = engine.outline_result(".").unwrap();
    let paths: Vec<_> = tree.items.iter().filter_map(|i| i.path.clone()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn a_file_outline_lists_imports_first_then_symbols_in_source_order() {
    let (_d, engine) = engine_in(IMPORTS_FIXTURE, "p");
    let outline = engine.outline_result("rust_crate/src/user.rs").unwrap();
    let roles: Vec<_> = outline.items.iter().map(|i| i.role.as_deref()).collect();
    let first_symbol = roles.iter().position(|r| *r != Some("Import")).unwrap();
    assert!(first_symbol > 0 && roles[first_symbol..].iter().all(|r| *r != Some("Import")));
    let lines: Vec<_> = outline.items[first_symbol..]
        .iter()
        .map(|i| i.line)
        .collect();
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted);
    // Imports by the imported node's path.
    let paths: Vec<_> = outline.items[..first_symbol]
        .iter()
        .map(|i| i.path.clone().unwrap())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

// --- Target selection ---

#[test]
fn plan_takes_the_first_match_by_path_then_line() {
    let (_d, engine) = engine_in(DOCS, "p");
    // `flush_buffer` is in src/audit/retention.rs and src/billing/invoice.py.
    let plan = engine.plan_result("flush_buffer", None);
    assert_eq!(plan.target_path.as_deref(), Some("src/audit/retention.rs"));
    // `RetentionPolicy` is a struct and an impl in retention.rs and a class in invoice.py.
    let policy = engine.plan_result("RetentionPolicy", None);
    assert_eq!(
        policy.target_path.as_deref(),
        Some("src/audit/retention.rs")
    );
    let line = policy.target_line.unwrap();
    let all_lines: Vec<usize> = engine
        .entity_list("RetentionPolicy", None, None)
        .items
        .iter()
        .filter(|i| i.path == "src/audit/retention.rs" && i.symbol == "RetentionPolicy")
        .map(|i| i.line)
        .collect();
    assert_eq!(Some(&line), all_lines.iter().min());
}

#[test]
fn fetch_and_blast_take_the_first_match_in_the_file_by_line() {
    let (_d, engine) = engine_in(IMPORTS_FIXTURE, "p");
    // Two `impl From<..> for E` blocks both hold a `from`.
    let fetched = engine
        .fetch_result("rust_crate/src/errors.rs", "from")
        .unwrap();
    assert_eq!(fetched.line, 4);
    assert!(fetched.source.contains("io::Error"), "{}", fetched.source);
    let found = engine
        .find_symbol_in_path("rust_crate/src/errors.rs", "from")
        .unwrap();
    assert_eq!(found.line, 4);
    let blast = engine.blast_result("rust_crate/src/errors.rs", "from", None);
    assert_eq!(blast.matched, BlastMatch::Symbol);
    assert_eq!(blast.target_line, Some(4));
    // `E` is the struct (line 1) and two impl blocks: the first by line is the struct.
    let e = engine
        .fetch_result("rust_crate/src/errors.rs", "E")
        .unwrap();
    assert_eq!(e.line, 1);
}

#[test]
fn the_json_of_an_ambiguous_target_names_it() {
    let (_d, engine) = engine_in(DOCS, "p");
    let v = serde_json::to_value(engine.plan_result("flush_buffer", None)).unwrap();
    assert_eq!(v["target_path"], json!("src/audit/retention.rs"));
    assert!(v["target_line"].is_number());
}
