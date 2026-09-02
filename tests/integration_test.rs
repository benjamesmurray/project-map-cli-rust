use std::path::Path;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::toon::ToonFormatter;

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
    assert_eq!(users_table[0].kind, "database_table");
    
    let active_users_view = engine.find_symbols("active_users");
    assert!(!active_users_view.is_empty(), "Should find active_users view");
    assert_eq!(active_users_view[0].kind, "database_table");
    
    let count_fn = engine.find_symbols("get_user_count");
    assert!(!count_fn.is_empty(), "Should find get_user_count function");
    assert_eq!(count_fn[0].kind, "function");

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

#[test]
fn test_lua_support() {
    let root = Path::new("tests/fixtures/lua");
    let out = Path::new("tests/test-lua-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let add_fn = engine.find_symbols("add");
    assert!(!add_fn.is_empty(), "Should find add function");
    assert_eq!(add_fn[0].kind, "function");
    assert!(add_fn[0].docstring.as_ref().unwrap().contains("Adds two numbers"));

    let sub_fn = engine.find_symbols("subtract");
    assert!(!sub_fn.is_empty(), "Should find subtract function");
    assert_eq!(sub_fn[0].kind, "function");

    // 2. Check Imports/Blast Radius
    // module_a.lua requires "module_b"
    // The relative path in the index will be tests/fixtures/lua/module_b.lua
    let blast = engine.check_blast_radius("tests/fixtures/lua/module_b.lua", "");
    let found_a = blast.iter().any(|n| n.path.contains("module_a.lua"));
    assert!(found_a, "Blast radius of module_b.lua should include module_a.lua");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_php_symfony_support() {
    let root = Path::new("tests/fixtures/php_symfony");
    let out = Path::new("tests/test-php-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check PHP Symbols
    let controller = engine.find_symbols("DefaultController");
    assert!(!controller.is_empty(), "Should find DefaultController class");
    // Ensure DefaultController is correctly identified as a class
    assert!(controller.iter().any(|s| s.kind == "class"), "Should find DefaultController as a class");
    
    let index_fn = engine.find_symbols("index");
    assert!(!index_fn.is_empty(), "Should find index method");
    assert_eq!(index_fn[0].kind, "function");
    
    // Check if attributes/annotations are in docstring
    assert!(index_fn[0].docstring.as_ref().unwrap().contains("Route"), "Docstring should contain Route attribute/annotation");

    // 2. Check YAML Symbols
    let service_config = engine.find_symbols("App\\Service\\MyService");
    assert!(!service_config.is_empty(), "Should find service configuration in YAML");
    assert_eq!(service_config[0].kind, "config");

    // 3. Check Blast Radius via 'use' statement
    // DefaultController.php uses App\Service\MyService
    let blast = engine.check_blast_radius("tests/fixtures/php_symfony/src/Service/MyService.php", "");
    let found_controller = blast.iter().any(|n| n.path.contains("DefaultController.php"));
    assert!(found_controller, "Blast radius of MyService.php should include DefaultController.php via 'use' statement");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_terraform_support() {
    let root = Path::new("tests/fixtures/terraform");
    let out = Path::new("tests/test-tf-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let instance = engine.find_symbols("aws_instance.web");
    assert!(!instance.is_empty(), "Should find aws_instance.web resource");
    assert_eq!(instance[0].kind, "resource");
    
    let region_var = engine.find_symbols("region");
    assert!(!region_var.is_empty(), "Should find region variable");
    assert_eq!(region_var[0].kind, "variable");

    let network_mod = engine.find_symbols("network");
    assert!(!network_mod.is_empty(), "Should find network module");
    assert_eq!(network_mod[0].kind, "module");

    // 2. Check Module Dependencies / Blast Radius
    // main.tf uses module 'network' with source './modules/network'
    // The relative path indexed will be tests/fixtures/terraform/modules/network/main.tf
    let blast = engine.check_blast_radius("tests/fixtures/terraform/modules/network/main.tf", "");
    let found_main = blast.iter().any(|n| n.path.contains("main.tf"));
    assert!(found_main, "Blast radius of network module should include main.tf");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_gdscript_support() {
    let root = Path::new("tests/fixtures/gdscript");
    let out = Path::new("tests/test-gd-index.json");
    
    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");
    
    let engine = QueryEngine::load(out).expect("Failed to load index");
    
    // 1. Check Symbols
    let player_class = engine.find_symbols("Player");
    assert!(!player_class.is_empty(), "Should find Player class");
    assert_eq!(player_class[0].kind, "class");
    
    let signal_sym = engine.find_symbols("health_changed");
    assert!(!signal_sym.is_empty(), "Should find health_changed signal");
    assert_eq!(signal_sym[0].kind, "signal");

    let func_sym = engine.find_symbols("take_damage");
    assert!(!func_sym.is_empty(), "Should find take_damage function");
    assert_eq!(func_sym[0].kind, "function");

    // 2. Check Module Dependencies / Blast Radius via res:// and extends
    // player.gd preloads weapon.gd and extends item.gd
    let blast_weapon = engine.check_blast_radius("tests/fixtures/gdscript/weapon.gd", "");
    let found_player_w = blast_weapon.iter().any(|n| n.path.contains("player.gd"));
    assert!(found_player_w, "Blast radius of weapon.gd should include player.gd");

    let blast_item = engine.check_blast_radius("tests/fixtures/gdscript/item.gd", "");
    let found_player_i = blast_item.iter().any(|n| n.path.contains("player.gd"));
    assert!(found_player_i, "Blast radius of item.gd should include player.gd");

    std::fs::remove_file(out).ok();
}

#[test]
fn test_streaming_and_topic_indexing() {
    let root = Path::new("tests/fixtures/streaming");
    let out = Path::new("tests/test-streaming-index.json");

    let mut orch = Orchestrator::new();
    orch.build_index(root).expect("Failed to build index");
    orch.save_index(out).expect("Failed to save index");

    let engine = QueryEngine::load(out).expect("Failed to load index");

    // 1. Topic search
    let matches = engine.find_entities_with_preview("wde.labels.phase.v1", Some(2));
    assert!(!matches.is_empty(), "Should find wde.labels.phase.v1 topic");

    let producers: Vec<_> = matches.iter().filter(|m| m.role.as_deref() == Some("Producer")).collect();
    assert!(!producers.is_empty(), "Should find producer for wde.labels.phase.v1");
    let prod_preview = producers[0].preview.as_ref().expect("Producer should have preview");
    assert!(prod_preview.formatted.contains("wde.labels.phase.v1"));

    // 2. Database table search
    let table_matches = engine.find_symbols("public.levels_history");
    assert!(!table_matches.is_empty(), "Should find public.levels_history table");
    assert_eq!(table_matches[0].kind, "database_table");

    // 3. Environment variable search
    let env_matches = engine.find_symbols("DATABASE_URL");
    assert!(!env_matches.is_empty(), "Should find DATABASE_URL config variable");
    assert_eq!(env_matches[0].kind, "config_env_var");

    let pass_matches = engine.find_symbols("DB_PASSWORD");
    assert!(!pass_matches.is_empty(), "Should find DB_PASSWORD");

    // 4. Test ToonFormatter output
    let output = ToonFormatter::format_entity_matches("wde.labels.phase.v1", &matches);
    assert!(output.contains("Producers:"));
    assert!(output.contains("```"));

    std::fs::remove_file(out).ok();
}
