use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::sleep;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::watcher::{ProjectWatcher, is_ignored_path, is_permission_denied};

#[test]
fn test_watcher_ignores_meta_directories() {
    assert!(is_ignored_path(Path::new(".project-map/latest/.project-map.json")));
    assert!(is_ignored_path(Path::new(".git/HEAD")));
    assert!(is_ignored_path(Path::new("target/debug/deps")));
    assert!(is_ignored_path(Path::new("node_modules/express")));
    assert!(!is_ignored_path(Path::new("src/main.rs")));
    assert!(!is_ignored_path(Path::new("app/services/user.py")));
}

#[test]
fn test_permission_denied_detection() {
    let io_err = notify::Error::io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied (os error 13)"));
    assert!(is_permission_denied(&io_err));

    let generic_err = notify::Error::generic("Watcher watch error: Permission denied (os error 13) about [\"/opt/wde/db_data\"]");
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
    orch.build_index(&temp_dir).unwrap();
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
    let watcher = ProjectWatcher::new(temp_dir.clone(), out_dir.clone(), Arc::clone(&shared_engine));
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
        assert!(!matches.is_empty(), "Should auto-index new symbol added in background");
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
        let _ = std::fs::set_permissions(&unreadable_dir, std::fs::Permissions::from_mode(0000));
    }

    let mut orch = Orchestrator::new();
    orch.build_index(&temp_dir).unwrap();
    orch.save_index_versioned(&out_dir).unwrap();

    let latest = out_dir.join("latest").join(".project-map.json");
    let engine = QueryEngine::load(&latest).unwrap();
    let shared_engine = Arc::new(RwLock::new(Some(engine)));

    // Start background watcher - MUST NOT CRASH even with unreadable 0000 dir or ignored dirs
    let watcher = ProjectWatcher::new(temp_dir.clone(), out_dir.clone(), Arc::clone(&shared_engine));
    assert!(watcher.start_in_background().is_ok(), "Watcher should initialize gracefully despite unreadable/ignored paths");

    // Clean up Unix permissions before deleting directory
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable_dir = temp_dir.join("db_data");
        let _ = std::fs::set_permissions(&unreadable_dir, std::fs::Permissions::from_mode(0755));
    }

    std::fs::remove_dir_all(&temp_dir).ok();
}
