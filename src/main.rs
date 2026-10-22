use clap::Parser;
use project_map_cli_rust::cli::commands::{Cli, Commands, DocCommands};
use project_map_cli_rust::core::json_out::error_json;
use project_map_cli_rust::core::orchestrator::rebuild_and_publish;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::{IndexInfo, QueryError};
use project_map_cli_rust::core::tools::{run, Format, ToolCall, ToolEnv, ToolOutput};
use project_map_cli_rust::error::Result;
use project_map_cli_rust::mcp::server::McpServer;

fn get_index_path(index_dir: &str) -> std::path::PathBuf {
    std::path::Path::new(index_dir)
        .join("latest")
        .join(".project-map.json")
}

/// Prints the tool's output. In JSON mode an error is the error envelope on stdout and
/// a non-zero exit; TOON errors are printed and exit 0, as in 0.3.0.
fn print_output(out: &ToolOutput, format: Format) {
    use std::io::Write;
    println!("{}", out.text);
    let _ = std::io::stdout().flush();
    if format == Format::Json && out.is_error {
        std::process::exit(1);
    }
}

/// Loads the index and runs one query tool. A missing or unreadable index is an error
/// return in TOON, and the error envelope in JSON.
fn run_query(index: &str, call: &ToolCall<'_>, format: Format) -> Result<()> {
    let path = get_index_path(index);
    let env = ToolEnv {
        root: std::path::Path::new("."),
        out: std::path::Path::new(index),
    };
    let engine = match QueryEngine::load(&path) {
        Ok(engine) => Some(engine),
        Err(_) if !path.exists() => None,
        Err(e) if format == Format::Toon => return Err(e),
        Err(e) => {
            let value = error_json(
                call.name(),
                &QueryError::io_error(e.to_string()),
                &IndexInfo::missing(),
            );
            let out = ToolOutput {
                text: value.to_string(),
                json: Some(value),
                is_error: true,
            };
            print_output(&out, format);
            return Ok(());
        }
    };
    if engine.is_none() && format == Format::Toon {
        // A missing file: the loader's own error, as before.
        QueryEngine::load(&path)?;
    }
    print_output(&run(engine.as_ref(), env, call, format), format);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { root, out } | Commands::Refresh { root, out } => {
            // `PROJECT_MAP_LOG=debug` shows the build's debug log (ambiguous imports) on stderr.
            let level = match std::env::var("PROJECT_MAP_LOG").as_deref() {
                Ok("debug") => tracing::Level::DEBUG,
                _ => tracing::Level::WARN,
            };
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_max_level(level)
                .try_init();
            println!("Building project map index with rotation...");
            // Print the readable message (e.g. NewerIndex says what to do), not Debug.
            if let Err(e) =
                rebuild_and_publish(std::path::Path::new(&root), std::path::Path::new(&out))
            {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            println!("Index saved and versioned in {}", out);
        }
        Commands::Find {
            query,
            file,
            index,
            preview,
            limit,
            fmt,
        } if query.is_none() && file.is_none() && fmt.format == Format::Toon => {
            let _ = (index, preview, limit);
            println!("Error: Provide --query or --file");
        }
        Commands::Find {
            query,
            file,
            index,
            preview,
            limit,
            fmt,
        } => run_query(
            &index,
            &ToolCall::Query {
                query: query.as_deref(),
                path: None,
                filename: if query.is_some() {
                    None
                } else {
                    file.as_deref()
                },
                preview,
                limit,
            },
            fmt.format,
        )?,
        Commands::Context { path, index, fmt } => run_query(
            &index,
            &ToolCall::Query {
                query: None,
                path: Some(&path),
                filename: None,
                preview: None,
                limit: None,
            },
            fmt.format,
        )?,
        Commands::Impact {
            fqn,
            index,
            limit,
            fmt,
        } => run_query(
            &index,
            &ToolCall::Plan {
                symbol: &fqn,
                limit,
            },
            fmt.format,
        )?,
        Commands::Status {
            index,
            max_depth,
            fmt,
        } => {
            let engine = QueryEngine::load_with_retry(
                &get_index_path(&index),
                project_map_cli_rust::core::query_engine::RetryPolicy::default(),
            )
            .ok();
            let env = ToolEnv {
                root: std::path::Path::new("."),
                out: std::path::Path::new(&index),
            };
            let out = run(
                engine.as_ref(),
                env,
                &ToolCall::Status { max_depth },
                fmt.format,
            );
            print_output(&out, fmt.format);
        }
        Commands::Fetch {
            path,
            symbol,
            index,
            fmt,
        } => run_query(
            &index,
            &ToolCall::FetchSymbol {
                path: &path,
                symbol: &symbol,
            },
            fmt.format,
        )?,
        Commands::Blast {
            path,
            symbol,
            index,
            limit,
            fmt,
        } => run_query(
            &index,
            &ToolCall::BlastRadius {
                path: &path,
                symbol: &symbol,
                limit,
            },
            fmt.format,
        )?,
        Commands::Search {
            query,
            index,
            preview,
            limit,
            fmt,
        } => run_query(
            &index,
            &ToolCall::Search {
                query: &query,
                preview,
                limit,
            },
            fmt.format,
        )?,
        Commands::Doc {
            index,
            format,
            command,
        } => {
            let call = match &command {
                DocCommands::Search { query, limit } => ToolCall::DocSearch {
                    query,
                    limit: Some(*limit),
                },
                DocCommands::Outline { path, max_depth } => ToolCall::DocOutline {
                    path,
                    max_depth: *max_depth,
                },
                DocCommands::Section {
                    path,
                    anchor,
                    max_chars,
                } => ToolCall::DocSection {
                    path,
                    anchor,
                    max_chars: *max_chars,
                },
                DocCommands::Links { path, symbol } => ToolCall::DocLinks {
                    path,
                    symbol: symbol.as_deref(),
                },
                DocCommands::Terms { path_prefix, limit } => ToolCall::DocTerms {
                    path_prefix: path_prefix.as_deref(),
                    limit: Some(*limit),
                },
            };
            run_query(&index, &call, format)?;
        }
        Commands::Api { path, index, fmt } => run_query(
            &index,
            &ToolCall::ApiSurface {
                path: path.as_deref(),
            },
            fmt.format,
        )?,
        Commands::Modules {
            path_prefix,
            max_depth,
            index,
            fmt,
        } => run_query(
            &index,
            &ToolCall::Modules {
                path_prefix: path_prefix.as_deref(),
                max_depth: Some(max_depth),
            },
            fmt.format,
        )?,
        Commands::Imports {
            package,
            path_prefix,
            limit,
            index,
            fmt,
        } => run_query(
            &index,
            &ToolCall::Imports {
                package: &package,
                path_prefix: path_prefix.as_deref(),
                limit: Some(limit),
            },
            fmt.format,
        )?,
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
