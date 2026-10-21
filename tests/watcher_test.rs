use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::watcher::{is_ignored_path, is_permission_denied, ProjectWatcher};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::sleep;

#[test]
fn test_watcher_ignores_meta_directories() {
    assert!(is_ignored_path(
        Path::new(".project-map/latest/.project-map.json"),
        None
    ));
    assert!(is_ignored_path(Path::new(".git/HEAD"), None));
    assert!(is_ignored_path(Path::new("target/debug/deps"), None));
    assert!(is_ignored_path(Path::new("node_modules/express"), None));
    assert!(!is_ignored_path(Path::new("src/main.rs"), None));
    assert!(!is_ignored_path(Path::new("app/services/user.py"), None));
}

#[test]
fn test_permission_denied_detection() {
    let io_err = notify::Error::io(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "Permission denied (os error 13)",
    ));
    assert!(is_permission_denied(&io_err));

    let generic_err = notify::Error::generic(
        "Watcher watch error: Permission denied (os error 13) about [\"/opt/wde/db_data\"]",
    );
    assert!(is_permission_denied(&generic_err));

    let other_err = notify::Error::generic("No space left on device");
    assert!(!is_permission_denied(&other_err));
}

#[tokio::test]
async fn test_watcher_auto_reindex_on_file_change() {
    let temp_dir = std::env::temp_dir().join("test_watcher_workspace");
    let out_dir = temp_dir.join(".project-map");

    // Clean previous run
    std::fs::remove_dir_all(&temp_dir).ok();
    std::fs::create_dir_all(&temp_dir).unwrap();

    let initial_file = temp_dir.join("main.py");
    std::fs::write(&initial_file, "def initial_function():\n    pass\n").unwrap();

    let mut orch = Orchestrator::new();
    orch.build_index(&temp_dir, &out_dir).unwrap();
    orch.save_index_versioned(&out_dir).unwrap();

    let latest = out_dir.join("latest").join(".project-map.json");
    let engine = QueryEngine::load(&latest).unwrap();
    let shared_engine = Arc::new(RwLock::new(Some(engine)));

    // Verify initial state
    {
        let lock = shared_engine.read().unwrap();
        let engine_ref = lock.as_ref().unwrap();
        assert!(!engine_ref.find_symbols("initial_function").is_empty());
        assert!(engine_ref.find_symbols("auto_indexed_function").is_empty());
    }

    // Start background watcher
    let watcher = ProjectWatcher::new(
        temp_dir.clone(),
        out_dir.clone(),
        Arc::clone(&shared_engine),
    );
    watcher.start_in_background().unwrap();

    // Create a new source file
    sleep(Duration::from_millis(100)).await;
    let new_file = temp_dir.join("new_module.py");
    std::fs::write(&new_file, "def auto_indexed_function():\n    pass\n").unwrap();

    // Wait for debounce (500ms) + processing time
    sleep(Duration::from_millis(1000)).await;

    // Verify background auto-indexing updated the shared engine
    {
        let lock = shared_engine.read().unwrap();
        let engine_ref = lock.as_ref().unwrap();
        let matches = engine_ref.find_symbols("auto_indexed_function");
        assert!(
            !matches.is_empty(),
            "Should auto-index new symbol added in background"
        );
    }

    // Clean up
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_watcher_handles_gitignore_and_unreadable_dirs() {
    let temp_dir = std::env::temp_dir().join("test_watcher_gitignore_perm");
    let out_dir = temp_dir.join(".project-map");

    std::fs::remove_dir_all(&temp_dir).ok();
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Create .gitignore
    std::fs::write(temp_dir.join(".gitignore"), "ignored_dir/\n").unwrap();

    // Create ignored dir and regular dir
    let ignored_dir = temp_dir.join("ignored_dir");
    std::fs::create_dir_all(&ignored_dir).unwrap();
    std::fs::write(ignored_dir.join("secret.py"), "def secret_func(): pass\n").unwrap();

    let app_dir = temp_dir.join("app");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::write(app_dir.join("server.py"), "def app_server(): pass\n").unwrap();

    // On Unix, test creating a restricted directory (mode 0000)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable_dir = temp_dir.join("db_data");
        let _ = std::fs::create_dir_all(&unreadable_dir);
        let _ = std::fs::set_permissions(&unreadable_dir, std::fs::Permissions::from_mode(0o0000));
    }

    let mut orch = Orchestrator::new();
    orch.build_index(&temp_dir, &out_dir).unwrap();
    orch.save_index_versioned(&out_dir).unwrap();

    let latest = out_dir.join("latest").join(".project-map.json");
    let engine = QueryEngine::load(&latest).unwrap();
    let shared_engine = Arc::new(RwLock::new(Some(engine)));

    // Start background watcher - MUST NOT CRASH even with unreadable 0000 dir or ignored dirs
    let watcher = ProjectWatcher::new(
        temp_dir.clone(),
        out_dir.clone(),
        Arc::clone(&shared_engine),
    );
    assert!(
        watcher.start_in_background().is_ok(),
        "Watcher should initialize gracefully despite unreadable/ignored paths"
    );

    // Clean up Unix permissions before deleting directory
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable_dir = temp_dir.join("db_data");
        let _ = std::fs::set_permissions(&unreadable_dir, std::fs::Permissions::from_mode(0o0755));
    }

    std::fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_build_index_root_equals_out_dir_errors() {
    let temp = tempfile::tempdir().unwrap();
    let out_dir = temp.path();
    let mut orch = Orchestrator::new();
    let res = orch.build_index(out_dir, out_dir);
    assert!(res.is_err(), "build_index(out, out) must error");

    let scaffold_res = orch.scaffold_if_empty(out_dir, out_dir);
    assert!(
        scaffold_res.is_err(),
        "scaffold_if_empty(out, out) must error"
    );
}

#[test]
fn test_custom_out_dir_inside_root_is_not_indexed_and_ignored_by_watcher() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let custom_out = root.join("my_custom_out");
    std::fs::create_dir_all(&custom_out).unwrap();

    // Create a tracked file in root
    let main_file = root.join("main.py");
    std::fs::write(&main_file, "def main_fn(): pass\n").unwrap();

    // Create a file inside custom_out that would otherwise match supported extensions
    let out_file = custom_out.join("generated.py");
    std::fs::write(&out_file, "def generated_fn(): pass\n").unwrap();

    let mut orch = Orchestrator::new();
    orch.build_index(root, &custom_out)
        .expect("build index with custom out");
    orch.save_index(&custom_out.join("index.json"))
        .expect("save index");

    let engine = QueryEngine::load(&custom_out.join("index.json")).expect("load index");
    assert!(
        !engine.find_symbols("main_fn").is_empty(),
        "main_fn should be indexed"
    );
    assert!(
        engine.find_symbols("generated_fn").is_empty(),
        "files in custom out_dir must not be indexed"
    );

    // Check watcher ignore logic
    assert!(
        is_ignored_path(&out_file, Some(&custom_out)),
        "custom out file must be ignored by watcher"
    );
    assert!(
        !is_ignored_path(&main_file, Some(&custom_out)),
        "main file must not be ignored by watcher"
    );
}

#[tokio::test]
async fn test_watcher_reload_failure_retains_previous_engine() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let out_dir = root.join(".project-map");

    let initial_file = root.join("main.py");
    std::fs::write(&initial_file, "def existing_symbol(): pass\n").unwrap();

    let mut orch = Orchestrator::new();
    orch.build_index(&root, &out_dir).unwrap();
    orch.save_index_versioned(&out_dir).unwrap();

    let latest = out_dir.join("latest").join(".project-map.json");
    let engine = QueryEngine::load(&latest).unwrap();
    let shared_engine = Arc::new(RwLock::new(Some(engine)));

    // Verify existing symbol present
    {
        let lock = shared_engine.read().unwrap();
        assert!(!lock
            .as_ref()
            .unwrap()
            .find_symbols("existing_symbol")
            .is_empty());
    }

    // Start watcher with short lock timeout
    let watcher = ProjectWatcher::new(root.clone(), out_dir.clone(), Arc::clone(&shared_engine))
        .with_lock_timeout(Duration::from_millis(500));
    watcher.start_in_background().unwrap();

    // Now write a file change to trigger watcher
    std::fs::write(root.join("new_mod.py"), "def new_mod_sym(): pass\n").unwrap();

    // Give it a moment to run
    sleep(Duration::from_millis(1500)).await;

    // Both existing and new symbol should be available
    {
        let lock = shared_engine.read().unwrap();
        let eng = lock.as_ref().unwrap();
        assert!(!eng.find_symbols("existing_symbol").is_empty());
        assert!(!eng.find_symbols("new_mod_sym").is_empty());
    }
}

/// Task 4.2 / Criterion 4: Multi-Process Watcher Stress Test
/// Spawns two child processes of the built binary on one temp root with 200 files,
/// performs 200 rapid writes (including byte-identical rewrites of a JSON file),
/// while a concurrent polling reader asserts zero parse errors and no degenerate index.
#[test]
fn test_multi_process_watcher_stress_test() {
    use std::fs;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

    let temp = tempdir().expect("Failed to create tempdir");
    let root = temp.path().join("project");
    let out_dir = root.join(".project-map");
    fs::create_dir_all(&root).unwrap();

    // 1. Create 200 files, including a JSON file (the original reproduction trigger)
    let json_file = root.join("tracked_config.json");
    let json_content = r#"{"name": "wde", "active": true, "version": "1.0.0"}"#;
    fs::write(&json_file, json_content).unwrap();

    for i in 1..200 {
        let pkg = root.join(format!("pkg_{}", i % 10));
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join(format!("mod_{}.py", i)),
            format!("def func_{}():\n    return {}\n", i, i),
        )
        .unwrap();
    }

    // 2. Initial build using Orchestrator
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &out_dir).unwrap();
    orch.save_index_versioned(&out_dir).unwrap();

    let exe_path = env!("CARGO_BIN_EXE_project-map-cli-rust");

    // 3. Spawn two child processes running `watch --root <root> --out <out_dir>`
    let mut child1 = Command::new(exe_path)
        .arg("watch")
        .arg("--root")
        .arg(&root)
        .arg("--out")
        .arg(&out_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn watcher child 1");

    let mut child2 = Command::new(exe_path)
        .arg("watch")
        .arg("--root")
        .arg(&root)
        .arg("--out")
        .arg(&out_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn watcher child 2");

    // Ensure children are killed on exit even if assertion fails
    struct ChildGuard<'a>(&'a mut std::process::Child, &'a mut std::process::Child);
    impl<'a> Drop for ChildGuard<'a> {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
            let _ = self.1.kill();
            let _ = self.1.wait();
        }
    }
    let _guard = ChildGuard(&mut child1, &mut child2);

    // Wait a brief moment for watchers to initialise inotify
    std::thread::sleep(Duration::from_millis(500));

    let latest_file = out_dir.join("latest").join(".project-map.json");
    let done = Arc::new(AtomicBool::new(false));
    let done_reader = Arc::clone(&done);
    let latest_file_reader = latest_file.clone();

    // 4. Concurrent reader thread polling latest
    let reader_handle = std::thread::spawn(move || {
        let mut reads = 0;
        while !done_reader.load(Ordering::Relaxed) {
            if latest_file_reader.exists() {
                match QueryEngine::load(&latest_file_reader) {
                    Ok(engine) => {
                        let count = engine.get_file_count();
                        assert!(
                            count >= 190,
                            "Polling reader saw degenerate index: {} files (< 190 / 200)",
                            count
                        );
                        reads += 1;
                    }
                    Err(e) => {
                        panic!("Polling reader saw parse/corrupt error: {:?}", e);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        reads
    });

    // 5. Perform 200 rapid writes across files (including byte-identical writes to the JSON file)
    for i in 0..200 {
        if i % 3 == 0 {
            // Byte-identical rewrite of tracked_config.json (the exact reproduction probe)
            fs::write(&json_file, json_content).unwrap();
        } else {
            let file_idx = (i % 199) + 1;
            let pkg = root.join(format!("pkg_{}", file_idx % 10));
            fs::write(
                pkg.join(format!("mod_{}.py", file_idx)),
                format!("def func_{}():\n    return {}\n", file_idx, i),
            )
            .unwrap();
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    // Wait 2 seconds for watcher debounce and processing to settle
    std::thread::sleep(Duration::from_secs(2));

    done.store(true, Ordering::Relaxed);
    let reads = reader_handle.join().expect("Polling reader panicked");
    assert!(
        reads > 0,
        "Polling reader must have performed successful reads"
    );

    // 6. Assertions on final state:
    // (a) latest exists and loads cleanly
    let final_engine = QueryEngine::load(&latest_file).expect("Final latest must load cleanly");
    // (b) final latest contains all 200 files
    assert_eq!(
        final_engine.get_file_count(),
        200,
        "Final latest must contain all 200 files"
    );

    // (c) at most 5 backups
    let backups_dir = out_dir.join("backups");
    if backups_dir.exists() {
        let backup_count = fs::read_dir(&backups_dir).expect("read backups").count();
        assert!(
            backup_count <= 5,
            "Must keep at most 5 backups, but found {}",
            backup_count
        );
    }

    // (d) no ARCHITECTURE.md in out_dir
    assert!(
        !out_dir.join("ARCHITECTURE.md").exists(),
        "ARCHITECTURE.md must never be created in out_dir"
    );
}

/// Task 1.5 (Verification 3b): a watcher in front of a newer-schema index logs,
/// skips the rebuild, leaves the index byte-identical and keeps its engine.
#[tokio::test]
async fn test_watcher_skips_rebuild_over_newer_index() {
    use project_map_cli_rust::core::graph::SCHEMA_VERSION;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let out_dir = root.join(".project-map");
    std::fs::write(root.join("main.py"), "def local_symbol(): pass\n").unwrap();

    let latest_dir = out_dir.join("latest");
    std::fs::create_dir_all(&latest_dir).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/compatibility/v0_2_0_index.json").unwrap(),
    )
    .unwrap();
    json["meta"]["schema_version"] = serde_json::json!(SCHEMA_VERSION + 1);
    let latest = latest_dir.join(".project-map.json");
    std::fs::write(&latest, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    let before = std::fs::read(&latest).unwrap();

    let engine = QueryEngine::load(&latest).unwrap();
    let symbols_before = engine.get_symbol_count();
    let shared_engine = Arc::new(RwLock::new(Some(engine)));

    let watcher = ProjectWatcher::new(root.clone(), out_dir.clone(), Arc::clone(&shared_engine))
        .with_lock_timeout(Duration::from_millis(500));
    watcher.start_in_background().unwrap();

    std::fs::write(root.join("new_mod.py"), "def new_mod_sym(): pass\n").unwrap();
    sleep(Duration::from_millis(1500)).await;

    assert_eq!(
        std::fs::read(&latest).unwrap(),
        before,
        "newer index must be byte-identical"
    );
    assert!(
        !out_dir.join("backups").exists(),
        "no versioned build may be written"
    );
    let lock = shared_engine.read().unwrap();
    let eng = lock.as_ref().expect("engine retained");
    assert_eq!(eng.get_symbol_count(), symbols_before);
    assert!(eng.find_symbols("new_mod_sym").is_empty());
}
