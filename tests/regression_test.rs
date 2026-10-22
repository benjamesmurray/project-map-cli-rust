use project_map_cli_rust::core::graph::NodeType;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::toon::ToonFormatter;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

use project_map_cli_rust::core::query_engine::RetryPolicy;
use project_map_cli_rust::error::AppError;

/// (a) D1/D2: When an index file is corrupt/truncated, loading it must NOT
/// rebuild against .project-map, must NOT write ARCHITECTURE.md into out_dir,
/// must NOT create backup dirs, and must NOT overwrite latest.
#[test]
fn test_regression_d1_d2_corrupt_index_does_not_wipe_or_write() {
    let temp = tempdir().expect("Failed to create tempdir");
    let root = temp.path();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("Failed to create src dir");
    fs::write(
        src_dir.join("main.rs"),
        "fn main() { println!(\"hello\"); }",
    )
    .expect("Failed to write source file");

    let out_dir = root.join(".project-map");
    fs::create_dir_all(&out_dir).expect("Failed to create out dir");

    let mut orch = Orchestrator::new();
    orch.build_index(root, &out_dir)
        .expect("Failed to build index");
    orch.save_index_versioned(&out_dir)
        .expect("Failed to save versioned index");

    let latest_file = out_dir.join("latest").join(".project-map.json");
    assert!(latest_file.exists(), "latest index file must exist");

    let backups_dir = out_dir.join("backups");
    let backup_count_before = fs::read_dir(&backups_dir).expect("read backups").count();

    // Corrupt the latest index file by truncating it
    let corrupted_content = "{\"nodes\": [";
    fs::write(&latest_file, corrupted_content).expect("Failed to write corrupted content");

    // Attempt to load the index
    let load_result = QueryEngine::load(&latest_file);

    // D1/D2 expectations:
    // 1. Loading must return CorruptIndex error (pure reader)
    assert!(
        matches!(load_result, Err(AppError::CorruptIndex { .. })),
        "Loading a corrupted index must return CorruptIndex error, got: {:?}",
        load_result
    );

    // Also verify load_with_retry returns CorruptIndex
    let retry_policy = RetryPolicy::new(
        2,
        vec![
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(5),
        ],
    );
    let retry_result = QueryEngine::load_with_retry(&latest_file, retry_policy);
    assert!(
        matches!(retry_result, Err(AppError::CorruptIndex { .. })),
        "load_with_retry on corrupted index must return CorruptIndex error, got: {:?}",
        retry_result
    );

    // 2. Must not create ARCHITECTURE.md in out_dir (.project-map)
    assert!(
        !out_dir.join("ARCHITECTURE.md").exists(),
        "Reader must not scaffold ARCHITECTURE.md into out_dir"
    );

    // 3. Must not create new backup dirs
    let backup_count_after = fs::read_dir(&backups_dir).expect("read backups").count();
    assert_eq!(
        backup_count_before, backup_count_after,
        "Reader must not create new backup directories"
    );

    // 4. latest file must not have been overwritten with a newly built index
    let content_after = fs::read_to_string(&latest_file).expect("read latest after");
    assert_eq!(
        content_after, corrupted_content,
        "Reader must not overwrite latest file with a rebuilt index"
    );
}

/// (b) D5: Path normalization - query/fetch_symbol/blast_radius with 'a/b.py' vs './a/b.py' vs absolute
/// must give identical results.
#[test]
fn test_regression_d5_path_normalization_query_variants() {
    let root = Path::new(".");
    let temp = tempdir().expect("Failed to create tempdir");
    let out = temp.path().join("index.json");

    let mut orch = Orchestrator::new();
    orch.build_index(root, temp.path())
        .expect("Failed to build index");
    orch.save_index(&out).expect("Failed to save index");

    let engine = QueryEngine::load(&out).expect("Failed to load index");

    // Relative without ./, relative with ./, and canonical absolute path
    let rel_no_dot = "tests/fixtures/python/module_a.py";
    let rel_with_dot = "./tests/fixtures/python/module_a.py";
    let abs_path = fs::canonicalize(rel_with_dot)
        .expect("canonicalize")
        .to_string_lossy()
        .to_string();

    let outline_no_dot = engine.get_file_outline(rel_no_dot);
    let outline_with_dot = engine.get_file_outline(rel_with_dot);
    let outline_abs = engine.get_file_outline(&abs_path);

    assert!(
        !outline_no_dot.is_empty(),
        "Query with 'tests/fixtures/python/module_a.py' must return outline"
    );
    assert_eq!(
        outline_no_dot.len(),
        outline_with_dot.len(),
        "Query with and without './' must yield identical outline counts"
    );
    assert_eq!(
        outline_no_dot.len(),
        outline_abs.len(),
        "Query with absolute path must yield identical outline counts"
    );

    let symbol_no_dot = engine.find_symbol_in_path(rel_no_dot, "main");
    let symbol_with_dot = engine.find_symbol_in_path(rel_with_dot, "main");
    let symbol_abs = engine.find_symbol_in_path(&abs_path, "main");

    assert!(
        symbol_no_dot.is_some(),
        "find_symbol_in_path without './' must find symbol"
    );
    assert_eq!(
        symbol_no_dot.as_ref().map(|s| &s.name),
        symbol_with_dot.as_ref().map(|s| &s.name)
    );
    assert_eq!(
        symbol_no_dot.as_ref().map(|s| &s.name),
        symbol_abs.as_ref().map(|s| &s.name)
    );
}

/// (c) D7: File outline returns the file's own node and labels imports distinctly.
#[test]
fn test_regression_d7_file_outline_node_identity_and_imports() {
    let root = Path::new("tests/fixtures");
    let temp = tempdir().expect("Failed to create tempdir");
    let out = temp.path().join("index.json");

    let mut orch = Orchestrator::new();
    orch.build_index(root, temp.path())
        .expect("Failed to build index");
    orch.save_index(&out).expect("Failed to save index");

    let engine = QueryEngine::load(&out).expect("Failed to load index");

    let outline = engine.get_file_outline("tests/fixtures/python/module_a.py");
    for node in &outline {
        println!(
            "OUTLINE NODE: name={}, kind={}, line={}, node_type={:?}, role={:?}",
            node.name, node.kind, node.line, node.node_type, node.role
        );
    }
    let imported_file_nodes: Vec<_> = outline
        .iter()
        .filter(|n| n.node_type == NodeType::File)
        .collect();
    assert!(
        !imported_file_nodes.is_empty()
            && imported_file_nodes
                .iter()
                .all(|n| n.role.as_deref() == Some("Import")),
        "Imported files in outline must be labelled distinctly with role 'Import'"
    );

    // Verify format_file_context distinguishes imports
    let formatted =
        ToonFormatter::format_file_context("tests/fixtures/python/module_a.py", &outline);
    assert!(
        formatted.contains("- import "),
        "Formatted file context must format imported files with '- import ': {}",
        formatted
    );
}

/// Test covering two files with the same stem in different packages
#[test]
fn test_regression_d7_same_stem_different_packages() {
    let temp = tempdir().expect("Failed to create tempdir");
    let root = temp.path();

    // Create pkg_a/common.py and pkg_b/common.py
    let pkg_a = root.join("pkg_a");
    let pkg_b = root.join("pkg_b");
    fs::create_dir_all(&pkg_a).unwrap();
    fs::create_dir_all(&pkg_b).unwrap();

    fs::write(pkg_a.join("common.py"), "def helper_a(): pass\n").unwrap();
    fs::write(
        pkg_b.join("common.py"),
        "from pkg_a.common import helper_a\ndef helper_b(): pass\n",
    )
    .unwrap();

    let out_dir = root.join(".project-map");
    let mut orch = Orchestrator::new();
    orch.build_index(root, &out_dir).unwrap();
    orch.save_index_versioned(&out_dir).unwrap();

    let latest = out_dir.join("latest").join(".project-map.json");
    let engine = QueryEngine::load(&latest).unwrap();

    let outline_a = engine.get_file_outline("pkg_a/common.py");
    let helper_a_names: Vec<_> = outline_a.iter().map(|n| &n.name).collect();
    assert!(helper_a_names.contains(&&"helper_a".to_string()));

    let outline_b = engine.get_file_outline("pkg_b/common.py");
    let helper_b_names: Vec<_> = outline_b.iter().map(|n| &n.name).collect();
    assert!(helper_b_names.contains(&&"helper_b".to_string()));
}

/// (d) D6: Degenerate index (out-dir only / 0 project files) triggers a WARNING in status output.
#[test]
fn test_regression_d6_degenerate_index_status_warning() {
    // 6-node out-dir-only index observed during wipe reproduction
    let wiped_paths = vec![
        ".project-map/ARCHITECTURE.md".to_string(),
        ".project-map/backups/20261012_150854/.project-map.json".to_string(),
    ];

    let rendered_tree = project_map_cli_rust::core::utils::render_tree(&wiped_paths, 3);
    let status_output = ToonFormatter::format_status(
        true,
        Some(".project-map/latest/.project-map.json"),
        Some(&rendered_tree),
        &[],
    );

    assert!(
        status_output.contains("WARNING"),
        "Status output for degenerate index must contain WARNING, but got:\n{}",
        status_output
    );
}

/// Task 4.3: Compatibility with legacy v0.1.18 format index (./ paths, no meta).
/// Asserts that legacy index loads, paths are normalised in memory, and queries
/// answer both 'src/server.py' and './src/server.py'.
#[test]
fn test_compatibility_legacy_fixture_loads_and_answers_both_path_forms() {
    let fixture_path = Path::new("tests/fixtures/compatibility/legacy_v0_1_18_index.json");
    let engine = QueryEngine::load(fixture_path).expect("Legacy fixture must load cleanly");

    // Must load without meta
    assert!(engine.meta().is_none(), "Legacy fixture has no meta");

    // Answers outline for both path forms
    let outline_no_dot = engine.get_file_outline("src/server.py");
    let outline_dot = engine.get_file_outline("./src/server.py");
    assert!(
        !outline_no_dot.is_empty(),
        "Outline without dot must return symbols"
    );
    assert_eq!(
        outline_no_dot.len(),
        outline_dot.len(),
        "Both path forms must return same count"
    );

    // Answers find_symbol_in_path for both path forms
    let sym_no_dot = engine.find_symbol_in_path("src/server.py", "start_server");
    let sym_dot = engine.find_symbol_in_path("./src/server.py", "start_server");
    assert!(sym_no_dot.is_some(), "Symbol without dot must be found");
    assert_eq!(
        sym_no_dot.as_ref().map(|s| &s.name),
        sym_dot.as_ref().map(|s| &s.name)
    );
}

/// Task 4.3: Degenerate index fixture triggers status WARNING in format_full_status.
#[test]
fn test_compatibility_degenerate_fixture_triggers_status_warning() {
    let fixture_path = Path::new("tests/fixtures/compatibility/degenerate_index.json");
    let engine = QueryEngine::load(fixture_path).expect("Degenerate fixture must load cleanly");

    let status_output = ToonFormatter::format_full_status(
        Some(&engine),
        Some(Path::new("/opt/wde")),
        Some(Path::new("/opt/wde/.project-map")),
        Some(fixture_path.to_str().unwrap()),
        None,
        &[],
    );

    assert!(
        status_output.contains("WARNING"),
        "Status for degenerate fixture must emit WARNING: {}",
        status_output
    );
    assert!(
        status_output.contains("Index contains only output directory files"),
        "Status must warn about output directory only index: {}",
        status_output
    );
}

/// Snapshot of every file under `dir`: relative path, bytes and mtime.
fn snapshot_dir(dir: &Path) -> Vec<(String, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).expect("read_dir") {
            let p = entry.expect("entry").path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let meta = fs::metadata(&p).expect("metadata");
                out.push((
                    p.strip_prefix(dir).unwrap().to_string_lossy().to_string(),
                    fs::read(&p).expect("read"),
                    meta.modified().expect("mtime"),
                ));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Task 1.4 (Verification 3a): a schema-2 index and a legacy index without meta
/// load read-only, are reported stale by status, and are never rewritten by
/// status, query or search (library calls and the CLI binary).
#[test]
fn test_stale_index_detected_and_never_rewritten() {
    use project_map_cli_rust::core::graph::STALE_INDEX_MESSAGE;

    for fixture in ["v0_2_0_index.json", "legacy_v0_1_18_index.json"] {
        let temp = tempdir().expect("tempdir");
        let out_dir = temp.path().join(".project-map");
        let latest = out_dir.join("latest");
        fs::create_dir_all(&latest).unwrap();
        let index = latest.join(".project-map.json");
        fs::copy(
            Path::new("tests/fixtures/compatibility").join(fixture),
            &index,
        )
        .unwrap();
        let before = snapshot_dir(&out_dir);

        let engine = QueryEngine::load(&index).expect("stale index must still load");
        assert!(engine.is_stale(), "{fixture} must be reported stale");
        assert!(
            engine.get_file_count() > 0,
            "{fixture}: code data still served"
        );

        let status = ToonFormatter::format_full_status(
            Some(&engine),
            None,
            Some(&out_dir),
            index.to_str(),
            None,
            &[],
        );
        assert!(
            status.contains(STALE_INDEX_MESSAGE),
            "{fixture}: status must show the stale line, got:\n{status}"
        );

        // Code tools keep working against the stale index.
        let _ = engine.find_symbols("hello");
        let _ = engine.find_entities_with_preview("hello", None);
        for p in engine.get_all_file_paths() {
            let _ = engine.get_file_outline(&p);
        }

        // The CLI binary run against the same out dir must not write either.
        let exe = env!("CARGO_BIN_EXE_project-map-cli-rust");
        let idx_arg = out_dir.to_str().unwrap();
        for args in [
            vec!["status", "-i", idx_arg],
            vec!["find", "-q", "hello", "-i", idx_arg],
            vec!["search", "hello", "-i", idx_arg],
        ] {
            let res = std::process::Command::new(exe)
                .args(&args)
                .current_dir(temp.path())
                .output()
                .expect("run cli");
            if args[0] == "status" {
                let stdout = String::from_utf8_lossy(&res.stdout);
                assert!(
                    stdout.contains(STALE_INDEX_MESSAGE),
                    "{fixture}: CLI status must show the stale line, got:\n{stdout}"
                );
            }
        }

        assert_eq!(
            snapshot_dir(&out_dir),
            before,
            "{fixture}: out dir must be byte- and mtime-identical (no rewrite, backup or temp file)"
        );
    }
}

/// A freshly built index is current, and status does not call it stale.
#[test]
fn test_fresh_index_is_not_stale() {
    use project_map_cli_rust::core::graph::STALE_INDEX_MESSAGE;

    let temp = tempdir().expect("tempdir");
    let out = temp.path().join("index.json");
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new("tests/fixtures/python"), temp.path())
        .expect("build");
    orch.save_index(&out).expect("save");

    let engine = QueryEngine::load(&out).expect("load");
    assert!(!engine.is_stale());
    let status =
        ToonFormatter::format_full_status(Some(&engine), None, None, out.to_str(), None, &[]);
    assert!(!status.contains(STALE_INDEX_MESSAGE));
}

/// Task 2.1: a schema-3 index (0.3.0) loads read-only, is reported stale and
/// is not rewritten by status, query or search; its doc data still works.
#[test]
fn test_schema_3_index_is_stale_and_never_rewritten() {
    use project_map_cli_rust::core::graph::{OLD_SCHEMA_MESSAGE, STALE_INDEX_MESSAGE};

    let temp = tempdir().expect("tempdir");
    let out_dir = temp.path().join(".project-map");
    let index = seed_index_with_schema(&out_dir, 3);
    let before = snapshot_dir(&out_dir);

    let engine = QueryEngine::load(&index).expect("schema 3 must load");
    assert!(engine.is_stale());
    assert!(!engine.lacks_docs());
    assert_eq!(engine.meta().unwrap().git_commit, None);

    let status = ToonFormatter::format_full_status(
        Some(&engine),
        None,
        Some(&out_dir),
        index.to_str(),
        None,
        &[],
    );
    assert!(status.contains(OLD_SCHEMA_MESSAGE), "{status}");
    assert!(!status.contains(STALE_INDEX_MESSAGE), "{status}");

    let exe = env!("CARGO_BIN_EXE_project-map-cli-rust");
    let idx_arg = out_dir.to_str().unwrap();
    for args in [
        vec!["status", "-i", idx_arg],
        vec!["find", "-q", "hello", "-i", idx_arg],
        vec!["search", "hello", "-i", idx_arg],
    ] {
        let res = std::process::Command::new(exe)
            .args(&args)
            .current_dir(temp.path())
            .output()
            .expect("run cli");
        if args[0] == "status" {
            let stdout = String::from_utf8_lossy(&res.stdout);
            assert!(stdout.contains(OLD_SCHEMA_MESSAGE), "{stdout}");
        }
    }
    assert_eq!(
        snapshot_dir(&out_dir),
        before,
        "no rewrite, backup or temp file"
    );
}

/// Writes the schema-2 compatibility fixture with its schema_version replaced,
/// laid out as a real build does (`latest -> backups/<dir>`), and returns the index path.
fn seed_index_with_schema(out_dir: &Path, schema_version: u32) -> std::path::PathBuf {
    let backup = out_dir.join("backups").join("20261012_150000_000_1");
    fs::create_dir_all(&backup).unwrap();
    std::os::unix::fs::symlink(
        Path::new("backups").join("20261012_150000_000_1"),
        out_dir.join("latest"),
    )
    .unwrap();
    let latest = backup;
    let mut json: serde_json::Value = serde_json::from_str(
        &fs::read_to_string("tests/fixtures/compatibility/v0_2_0_index.json").unwrap(),
    )
    .unwrap();
    json["meta"]["schema_version"] = serde_json::json!(schema_version);
    let index = latest.join(".project-map.json");
    fs::write(&index, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    index
}

fn snapshot_without_lock(dir: &Path) -> Vec<(String, Vec<u8>, std::time::SystemTime)> {
    snapshot_dir(dir)
        .into_iter()
        .filter(|(p, _, _)| !p.ends_with(".lock"))
        .collect()
}

/// Task 1.5 (Verification 3b): building into an out dir holding a newer-schema
/// index fails with NewerIndex and leaves it untouched (library and CLI).
#[test]
fn test_forward_guard_refuses_to_overwrite_newer_index() {
    use project_map_cli_rust::core::graph::SCHEMA_VERSION;
    use project_map_cli_rust::core::orchestrator::rebuild_and_publish;

    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.py"), "def guarded(): pass\n").unwrap();
    let out_dir = root.join(".project-map");
    seed_index_with_schema(&out_dir, SCHEMA_VERSION + 1);
    let before = snapshot_without_lock(&out_dir);

    // rebuild_and_publish (CLI build/refresh path)
    match rebuild_and_publish(&root, &out_dir) {
        Err(AppError::NewerIndex { found, supported }) => {
            assert_eq!(found, SCHEMA_VERSION + 1);
            assert_eq!(found, 5, "schema 5 is the first newer than this build");
            assert_eq!(supported, SCHEMA_VERSION);
        }
        other => panic!("expected NewerIndex, got {:?}", other.map(|_| ())),
    }

    // build_index + save_index_versioned (library path)
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &out_dir)
        .expect("build itself does not write");
    assert!(matches!(
        orch.save_index_versioned(&out_dir),
        Err(AppError::NewerIndex { .. })
    ));

    // CLI binary exits non-zero and says why
    let res = std::process::Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
        .args([
            "build",
            "-r",
            root.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .expect("run cli");
    assert!(
        !res.status.success(),
        "CLI build must fail on a newer index"
    );
    let stderr = String::from_utf8_lossy(&res.stderr);
    assert!(
        stderr.contains("newer than this build supports"),
        "stderr: {stderr}"
    );

    assert_eq!(
        snapshot_without_lock(&out_dir),
        before,
        "newer index and out dir must be untouched"
    );
}

/// Older (v2) and current (v3) indexes are replaced normally.
#[test]
fn test_forward_guard_allows_older_and_current_index() {
    use project_map_cli_rust::core::graph::SCHEMA_VERSION;
    use project_map_cli_rust::core::orchestrator::rebuild_and_publish;

    for schema in [2, SCHEMA_VERSION] {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("main.py"), "def replaced(): pass\n").unwrap();
        let out_dir = root.join(".project-map");
        seed_index_with_schema(&out_dir, schema);

        let engine = rebuild_and_publish(&root, &out_dir)
            .unwrap_or_else(|e| panic!("schema {schema} must be replaced: {e}"));
        assert_eq!(engine.meta().unwrap().schema_version, SCHEMA_VERSION);
        assert!(!engine.find_symbols("replaced").is_empty());
    }
}
