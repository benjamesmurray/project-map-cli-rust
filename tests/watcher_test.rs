use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::sleep;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::watcher::{ProjectWatcher, is_ignored_path};

#[test]
fn test_watcher_ignores_meta_directories() {
    assert!(is_ignored_path(Path::new(".project-map/latest/.project-map.json")));
    assert!(is_ignored_path(Path::new(".git/HEAD")));
    assert!(is_ignored_path(Path::new("target/debug/deps")));
    assert!(is_ignored_path(Path::new("node_modules/express")));
    assert!(!is_ignored_path(Path::new("src/main.rs")));
    assert!(!is_ignored_path(Path::new("app/services/user.py")));
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
