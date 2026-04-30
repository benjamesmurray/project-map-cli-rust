use std::path::Path;
use clap::Parser;
use project_map_cli_rust::cli::commands::{Cli, Commands};
use project_map_cli_rust::error::Result;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::mcp::server::McpServer;

const INDEX_DIR: &str = ".project-map";
const INDEX_LATEST: &str = ".project-map/latest/.project-map.json";

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { root, out: _ } | Commands::Refresh { root, out: _ } => {
            println!("Building project map index with rotation...");
            let mut orch = Orchestrator::new();
            orch.build_index(Path::new(&root))?;
            orch.save_index_versioned(Path::new(INDEX_DIR))?;
            println!("Index saved and versioned in {}", INDEX_DIR);
        }
        Commands::Find { query } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let matches = engine.find_symbols(&query);
            
            println!("Resource: Symbols | Query: {}", query);
            println!("Matches Found: {}", matches.len());
            for m in matches.iter().take(10) {
                println!("- {} ({}) [line {}]", m.path, m.name, m.line);
            }
            if matches.len() > 10 {
                println!("... and {} more.", matches.len() - 10);
            }
        }
        Commands::Context { path } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let symbols = engine.get_file_outline(&path);
            
            println!("Resource: FileContext | Path: {}", path);
            println!("\n--- File Outline ---");
            for s in &symbols {
                println!("- {} {} (line: {})", s.kind, s.name, s.line);
            }
            if symbols.is_empty() {
                println!("- (No symbols detected or file not indexed)");
            }
        }
        Commands::Impact { fqn } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let impact = engine.analyze_impact(&fqn);
            
            println!("Resource: Impact Analysis | Target: {}", fqn);
            println!("Nodes Impacted: {}", impact.len());
            for node in impact.iter().take(10) {
                println!("- {:?}: {} ({})", node.node_type, node.name, node.path);
            }
        }
        Commands::Status => {
            println!("Project Map CLI - Status");
            if Path::new(INDEX_LATEST).exists() {
                let _engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
                println!("Phase: Ready");
                println!("Index: Found ({})", INDEX_LATEST);
            } else {
                println!("Phase: Discovery (No index found)");
                println!("Next Step: Run `project-map build` to generate the index.");
            }
        }
        Commands::Fetch { path, symbol } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            if let Some(node) = engine.find_symbol_in_path(&path, &symbol) {
                let content = std::fs::read_to_string(&node.path)?;
                let bytes = content.as_bytes();
                if node.start_byte < bytes.len() && node.end_byte <= bytes.len() {
                    let sub = &bytes[node.start_byte..node.end_byte];
                    println!("{}", String::from_utf8_lossy(sub));
                } else {
                    println!("Error: Byte range out of bounds for {}", path);
                }
            } else {
                println!("Resource: Fetch | Status: Symbol not found: {} in {}", symbol, path);
            }
        }
        Commands::Blast { path, symbol } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let results = engine.check_blast_radius(&path, &symbol);
            
            println!("Resource: Blast Radius | Symbol: {} in {}", symbol, path);
            if results.is_empty() {
                println!("No dependent components found.");
            } else {
                use std::collections::{HashMap, HashSet};
                
                let mut dir_counts: HashMap<String, usize> = HashMap::new();
                let mut unique_files: HashSet<String> = HashSet::new();

                for r in &results {
                    unique_files.insert(r.path.clone());
                    
                    // Extract project-relative subdirectory or package name
                    let path_parts: Vec<&str> = r.path.split('/').collect();
                    let group_key = if path_parts.len() > 6 {
                        path_parts[5..7].join("/")
                    } else if path_parts.len() > 1 {
                        path_parts[path_parts.len()-2..path_parts.len()-1].join("/")
                    } else {
                        "root".to_string()
                    };
                    *dir_counts.entry(group_key).or_insert(0) += 1;
                }

                println!("Summary:");
                println!("- Total Impacted Nodes: {}", results.len());
                println!("- Unique Files Affected: {}", unique_files.len());
                println!("- Affected Modules/Packages:");
                
                let mut sorted_dirs: Vec<_> = dir_counts.into_iter().collect();
                sorted_dirs.sort_by(|a, b| b.1.cmp(&a.1));
                
                for (dir, count) in sorted_dirs.iter().take(5) {
                    println!("  * {}: {} nodes", dir, count);
                }
                if sorted_dirs.len() > 5 {
                    println!("  * ... and {} more directories.", sorted_dirs.len() - 5);
                }

                println!("\nTop Direct Dependents:");
                for r in results.iter().take(10) {
                    println!("- {} (ln: {}) -> {}", r.path, r.line, r.name);
                }
                if results.len() > 10 {
                    println!("... and {} more nodes omitted for brevity.", results.len() - 10);
                }
            }
        }
        Commands::Search { query } => {
            println!("Searching for: {}", query);
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let matches = engine.find_symbols(&query);
            for m in matches {
                println!("- {}: {}", m.path, m.name);
            }
        }
        Commands::Mcp => {
            let server = McpServer::new();
            server.run().await?;
        }
    }

    Ok(())
}
