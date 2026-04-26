use std::path::Path;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;

#[test]
fn test_end_to_end_indexing() {
    let root = Path::new("tests/fixtures");
    let out = Path::new("tests/test-index.json");
    
    // 1. Build Index
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    // 2. Query Index
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // Check Python symbols
    let py_symbols = engine.find_symbols("Calculator");
    assert!(!py_symbols.is_empty(), "Should find Calculator class");
    assert_eq!(py_symbols[0].kind, "class");
    
    let py_fn = engine.find_symbols("global_function");
    assert!(!py_fn.is_empty(), "Should find global_function");
    assert_eq!(py_fn[0].kind, "function");

    // Check Rust symbols
    let rs_struct = engine.find_symbols("User");
    assert!(!rs_struct.is_empty(), "Should find User struct");
    assert_eq!(rs_struct[0].kind, "struct");
    
    let rs_trait = engine.find_symbols("Greeter");
    assert!(!rs_trait.is_empty(), "Should find Greeter trait");
    assert_eq!(rs_trait[0].kind, "trait");

    // Clean up
    std::fs::remove_file(out).ok();
}

#[test]
fn test_cross_file_dependencies() {
    let root = Path::new("tests/fixtures/python");
    let out = Path::new("tests/test-cross-file-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // module_a imports module_b.
    // Check if blast radius of module_b includes module_a.
    
    // We need the relative path as it's stored in the graph.
    // build_index uses path_to_fqn which results in "module_b" for module_b.py in the root.
    
    let blast = engine.check_blast_radius("tests/fixtures/python/module_b.py", "hello");
    
    // Blast radius should include module_a.py (via 'from module_b import hello')
    // and module_c.py (via 'import module_b')
    let found_a = blast.iter().any(|n| n.path.contains("module_a.py"));
    let found_c = blast.iter().any(|n| n.path.contains("module_c.py"));
    
    assert!(found_a, "Blast radius of module_b.hello should include module_a.py");
    assert!(found_c, "Blast radius of module_b.hello should include module_c.py");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_downstream_impact_analysis() {
    let root = Path::new("tests/fixtures/python");
    let out = Path::new("tests/test-impact-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // module_c depends on module_a and module_b
    let impact = engine.analyze_impact("module_c");
    
    let has_a = impact.iter().any(|n| n.path.contains("module_a.py"));
    let has_b = impact.iter().any(|n| n.path.contains("module_b.py"));
    
    assert!(has_a, "Impact of module_c should include module_a.py");
    assert!(has_b, "Impact of module_c should include module_b.py");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_typescript_support() {
    let root = Path::new("tests/fixtures/typescript");
    let out = Path::new("tests/test-ts-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let btn_symbols = engine.find_symbols("Button");
    assert!(!btn_symbols.is_empty(), "Should find Button class");
    assert!(btn_symbols.iter().any(|s| s.kind == "class"), "One of the matches should be a class");
    
    let props = engine.find_symbols("ButtonProps");
    assert!(!props.is_empty(), "Should find ButtonProps interface");
    assert!(props.iter().any(|s| s.kind == "interface"), "One of the matches should be an interface");

    // 2. Check Relative Imports
    // app.ts imports './components' which resolves to components/index.ts
    let blast = engine.check_blast_radius("tests/fixtures/typescript/components/index.ts", "");
    let found_app = blast.iter().any(|n| n.path.contains("app.ts"));
    assert!(found_app, "Blast radius of components/index.ts should include app.ts");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_kotlin_support() {
    let root = Path::new("tests/fixtures/kotlin");
    let out = Path::new("tests/test-kt-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let utils = engine.find_symbols("NetworkUtils");
    assert!(!utils.is_empty(), "Should find NetworkUtils class");
    assert!(utils.iter().any(|s| s.kind == "class"), "One of the matches should be a class");
    
    let helper = engine.find_symbols("helper");
    assert!(!helper.is_empty(), "Should find helper function");
    assert!(helper.iter().any(|s| s.kind == "function"), "One of the matches should be a function");

    // 2. Check Imports
    // Main.kt imports com.example.util.NetworkUtils
    let blast = engine.check_blast_radius("tests/fixtures/kotlin/com/example/util/NetworkUtils.kt", "");
    let found_main = blast.iter().any(|n| n.path.contains("Main.kt"));
    assert!(found_main, "Blast radius of NetworkUtils.kt should include Main.kt");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_sql_support() {
    let root = Path::new("tests/fixtures/sql");
    let out = Path::new("tests/test-sql-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let users_table = engine.find_symbols("users");
    assert!(!users_table.is_empty(), "Should find users table");
    assert_eq!(users_table[0].kind, "symbol");
    
    let active_users_view = engine.find_symbols("active_users");
    assert!(!active_users_view.is_empty(), "Should find active_users view");
    assert_eq!(active_users_view[0].kind, "symbol");
    
    let count_fn = engine.find_symbols("get_user_count");
    assert!(!count_fn.is_empty(), "Should find get_user_count function");
    assert_eq!(count_fn[0].kind, "symbol");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_vue_support() {
    let root = Path::new("tests/fixtures/vue");
    let out = Path::new("tests/test-vue-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let hello = engine.find_symbols("HelloWorld");
    assert!(!hello.is_empty(), "Should find HelloWorld component");
    assert_eq!(hello[0].kind, "component");
    
    let app = engine.find_symbols("App");
    assert!(!app.is_empty(), "Should find App component");
    assert_eq!(app[0].kind, "component");

    std::fs::remove_file(out).ok();
}
