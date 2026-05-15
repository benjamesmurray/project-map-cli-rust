use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "project-map")]
#[command(about = "Agent-Native architectural awareness CLI", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build or refresh the project map index
    Build {
        #[arg(short, long, default_value = ".")]
        root: String,
        #[arg(short, long, default_value = ".project-map")]
        out: String,
    },
    /// Alias for build
    Refresh {
        #[arg(short, long, default_value = ".")]
        root: String,
        #[arg(short, long, default_value = ".project-map")]
        out: String,
    },
    /// Find a symbol or file across the codebase
    Find {
        #[arg(short, long)]
        query: Option<String>,
        #[arg(short, long)]
        file: Option<String>,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Get a dense architectural overview of a specific file
    Context {
        #[arg(short, long)]
        path: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Analyze the architectural impact of a symbol
    Impact {
        #[arg(short, long)]
        fqn: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Returns current workspace context and available commands
    Status {
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Extract raw code for a specific symbol using AST parsing
    Fetch {
        #[arg(short, long)]
        path: String,
        #[arg(short, long)]
        symbol: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Check the blast radius (dependencies) of a symbol
    Blast {
        #[arg(short, long)]
        path: String,
        #[arg(short, long)]
        symbol: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Semantic keyword search over the codebase index
    Search {
        query: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Start the MCP server
    Mcp,
}
