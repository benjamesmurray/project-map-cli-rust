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
        /// Context snippet preview lines (default 3 when flag present)
        #[arg(short = 'C', long = "preview", num_args = 0..=1, default_missing_value = "3")]
        preview: Option<usize>,
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
        /// Context snippet preview lines (default 3 when flag present)
        #[arg(short = 'C', long = "preview", num_args = 0..=1, default_missing_value = "3")]
        preview: Option<usize>,
    },
    /// Query indexed documentation (Markdown, ADRs, specs)
    Doc {
        #[arg(short, long, default_value = ".project-map", global = true)]
        index: String,
        #[command(subcommand)]
        command: DocCommands,
    },
    /// List the operations and schemas declared in OpenAPI specs
    Api {
        /// Restrict the listing to one spec file
        path: Option<String>,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
    },
    /// Start the MCP server
    Mcp {
        #[arg(short, long)]
        watch: bool,
        #[arg(short, long, default_value = ".")]
        root: String,
        #[arg(short, long, default_value = ".project-map")]
        out: String,
    },
    /// Watch project directory and incrementally re-index on file changes
    Watch {
        #[arg(short, long, default_value = ".")]
        root: String,
        #[arg(short, long, default_value = ".project-map")]
        out: String,
    },
}

#[derive(Subcommand)]
pub enum DocCommands {
    /// Search documentation sections by keyword
    Search {
        query: String,
        /// Maximum number of sections to return
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Show a document's heading tree with anchors and frontmatter
    Outline { path: String },
    /// Read one section's raw text by path and anchor
    Section {
        path: String,
        anchor: String,
        /// Maximum characters of section text to return
        #[arg(long, default_value_t = 4000)]
        max_chars: usize,
    },
    /// Docs that mention a code file or symbol, or symbols a document references
    Links {
        path: String,
        /// Restrict a code file's links to one symbol
        #[arg(short, long)]
        symbol: Option<String>,
    },
    /// List recurring domain terms found in the documentation
    Terms {
        /// Only terms from documents under this path prefix
        path_prefix: Option<String>,
        /// Maximum number of terms to return
        #[arg(short, long, default_value_t = 30)]
        limit: usize,
    },
}
