use clap::Parser;
use project_map_cli_rust::cli::commands::{Cli, Commands};
use project_map_cli_rust::core::orchestrator::rebuild_and_publish;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::toon::ToonFormatter;
use project_map_cli_rust::core::utils::{get_active_features, normalize_rel_path, render_tree};
use project_map_cli_rust::error::Result;
use project_map_cli_rust::mcp::server::McpServer;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let get_index_path = |index_dir: &str| -> std::path::PathBuf {
        std::path::Path::new(index_dir)
            .join("latest")
            .join(".project-map.json")
    };

    match cli.command {
        Commands::Build { root, out } | Commands::Refresh { root, out } => {
            println!("Building project map index with rotation...");
            rebuild_and_publish(std::path::Path::new(&root), std::path::Path::new(&out))?;
            println!("Index saved and versioned in {}", out);
        }
        Commands::Find {
            query,
            file,
            index,
            preview,
        } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            if let Some(q) = query {
                let matches = engine.find_entities_with_preview(&q, preview);
                println!("{}", ToonFormatter::format_entity_matches(&q, &matches));
            } else if let Some(f) = file {
                let matches = engine.find_files(&f);
                println!("{}", ToonFormatter::format_file_matches(&f, &matches));
            } else {
                println!("Error: Provide --query or --file");
            }
        }
        Commands::Context { path, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let norm_path = normalize_rel_path(std::path::Path::new("."), &path);
            let symbols = engine.get_file_outline(&norm_path);
            println!(
                "{}",
                ToonFormatter::format_file_context(&norm_path, &symbols)
            );
        }
        Commands::Impact { fqn, index } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let impact = engine.analyze_impact(&fqn);
            println!("{}", ToonFormatter::format_impact_analysis(&fqn, &impact));
        }
        Commands::Status { index } => {
            let path = get_index_path(&index);
            let out_dir = std::path::Path::new(&index);
            let root_dir = std::path::Path::new(".");
            let engine = QueryEngine::load_with_retry(
                &path,
                project_map_cli_rust::core::query_engine::RetryPolicy::default(),
            )
            .ok();
            let (tree, active_features) = if let Some(ref eng) = engine {
                let paths = eng.get_all_file_paths();
                let tree = render_tree(&paths, 3);
                let active = get_active_features(root_dir);
                (Some(tree), active)
            } else {
                (None, Vec::new())
            };
            println!(
                "{}",
                ToonFormatter::format_full_status(
                    engine.as_ref(),
                    Some(root_dir),
                    Some(out_dir),
                    path.to_str(),
                    tree.as_deref(),
                    &active_features
                )
            );
        }
        Commands::Fetch {
            path,
            symbol,
            index,
        } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let norm_path = normalize_rel_path(std::path::Path::new("."), &path);
            if let Some(node) = engine.find_symbol_in_path(&norm_path, &symbol) {
                let content = std::fs::read_to_string(&node.path)?;
                let bytes = content.as_bytes();
                if node.start_byte < bytes.len() && node.end_byte <= bytes.len() {
                    let sub = &bytes[node.start_byte..node.end_byte];
                    let content_str = String::from_utf8_lossy(sub);
                    println!(
                        "{}",
                        ToonFormatter::format_fetch_result(&norm_path, &symbol, Some(&content_str))
                    );
                } else {
                    println!("Error: Byte range out of bounds for {}", norm_path);
                }
            } else {
                println!(
                    "{}",
                    ToonFormatter::format_fetch_result(&norm_path, &symbol, None)
                );
            }
        }
        Commands::Blast {
            path,
            symbol,
            index,
        } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let norm_path = normalize_rel_path(std::path::Path::new("."), &path);
            let results = engine.check_blast_radius(&norm_path, &symbol);
            println!(
                "{}",
                ToonFormatter::format_blast_radius(&norm_path, &symbol, &results)
            );
        }
        Commands::Search {
            query,
            index,
            preview,
        } => {
            let engine = QueryEngine::load(&get_index_path(&index))?;
            let matches = engine.find_entities_with_preview(&query, preview);
            println!("{}", ToonFormatter::format_entity_matches(&query, &matches));
        }
        Commands::Mcp { watch, root, out } => {
            let server = McpServer::with_paths(
                std::path::PathBuf::from(&root),
                std::path::PathBuf::from(&out),
            );
            if watch {
                let watcher = project_map_cli_rust::core::watcher::ProjectWatcher::new(
                    std::path::PathBuf::from(&root),
                    std::path::PathBuf::from(&out),
                    server.engine(),
                );
                watcher.start_in_background()?;
            }
            server.run().await?;
        }
        Commands::Watch { root, out } => {
            println!("Starting project-map file watcher for {}...", root);
            let root_path = std::path::PathBuf::from(&root);
            let out_path = std::path::PathBuf::from(&out);
            let engine = rebuild_and_publish(&root_path, &out_path).ok();
            let shared_engine = std::sync::Arc::new(std::sync::RwLock::new(engine));

            let watcher = project_map_cli_rust::core::watcher::ProjectWatcher::new(
                std::path::PathBuf::from(&root),
                std::path::PathBuf::from(&out),
                shared_engine,
            );
            watcher.start_in_background()?;

            println!("Watching for changes in {}. Press Ctrl+C to exit.", root);
            tokio::signal::ctrl_c().await.ok();
        }
    }

    Ok(())
}
