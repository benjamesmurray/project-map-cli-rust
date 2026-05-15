use clap::Parser;
use project_map_cli_rust::cli::commands::{Cli, Commands};
use project_map_cli_rust::error::Result;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::toon::ToonFormatter;
use project_map_cli_rust::mcp::server::McpServer;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let get_index_path = |index_dir: &str| -> std::path::PathBuf {
        std::path::Path::new(index_dir).join("latest").join(".project-map.json")
    };

    match cli.command {
        Commands::Build { root, out } | Commands::Refresh { root, out } => {
            println!("Building project map index with rotation...");
            let mut orch = Orchestrator::new();
            let _ = orch.scaffold_if_empty(std::path::Path::new(&root));
            orch.build_index(std::path::Path::new(&root))?;
            orch.save_index_versioned(std::path::Path::new(&out))?;
            println!("Index saved and versioned in {}", out);
        }
        Commands::Find { query, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let matches = engine.find_symbols(&query);
            println!("{}", ToonFormatter::format_symbols(&query, &matches));
        }
        Commands::Context { path, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let symbols = engine.get_file_outline(&path);
            println!("{}", ToonFormatter::format_file_context(&path, &symbols));
        }
        Commands::Impact { fqn, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let impact = engine.analyze_impact(&fqn);
            println!("{}", ToonFormatter::format_impact_analysis(&fqn, &impact));
        }
        Commands::Status { index } => {
            let path = get_index_path(&index);
            let is_ready = path.exists();
            println!("{}", ToonFormatter::format_status(is_ready, path.to_str()));
        }
        Commands::Fetch { path, symbol, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            if let Some(node) = engine.find_symbol_in_path(&path, &symbol) {
                let content = std::fs::read_to_string(&node.path)?;
                let bytes = content.as_bytes();
                if node.start_byte < bytes.len() && node.end_byte <= bytes.len() {
                    let sub = &bytes[node.start_byte..node.end_byte];
                    let content_str = String::from_utf8_lossy(sub);
                    println!("{}", ToonFormatter::format_fetch_result(&path, &symbol, Some(&content_str)));
                } else {
                    println!("Error: Byte range out of bounds for {}", path);
                }
            } else {
                println!("{}", ToonFormatter::format_fetch_result(&path, &symbol, None));
            }
        }
        Commands::Blast { path, symbol, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let results = engine.check_blast_radius(&path, &symbol);
            println!("{}", ToonFormatter::format_blast_radius(&path, &symbol, &results));
        }
        Commands::Search { query, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let matches = engine.find_symbols(&query);
            println!("{}", ToonFormatter::format_symbols(&query, &matches));
        }
        Commands::Mcp => {
            let server = McpServer::new();
            server.run().await?;
        }
    }

    Ok(())
}
