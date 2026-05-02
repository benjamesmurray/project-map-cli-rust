use std::path::Path;
use clap::Parser;
use project_map_cli_rust::cli::commands::{Cli, Commands};
use project_map_cli_rust::error::Result;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::toon::ToonFormatter;
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
            println!("{}", ToonFormatter::format_symbols(&query, &matches));
        }
        Commands::Context { path } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let symbols = engine.get_file_outline(&path);
            println!("{}", ToonFormatter::format_file_context(&path, &symbols));
        }
        Commands::Impact { fqn } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let impact = engine.analyze_impact(&fqn);
            println!("{}", ToonFormatter::format_impact_analysis(&fqn, &impact));
        }
        Commands::Status => {
            let is_ready = Path::new(INDEX_LATEST).exists();
            println!("{}", ToonFormatter::format_status(is_ready, Some(INDEX_LATEST)));
        }
        Commands::Fetch { path, symbol } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
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
        Commands::Blast { path, symbol } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
            let results = engine.check_blast_radius(&path, &symbol);
            println!("{}", ToonFormatter::format_blast_radius(&path, &symbol, &results));
        }
        Commands::Search { query } => {
            let engine = QueryEngine::load(Path::new(INDEX_LATEST))?;
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
