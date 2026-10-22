use crate::core::tools::Format;
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "project-map")]
#[command(about = "Agent-Native architectural awareness CLI", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// `--format`, on every query subcommand.
#[derive(Args, Debug, Clone, Copy)]
pub struct FormatArg {
    /// Output format: toon (default, compact text) or json (for programs, costs more tokens)
    #[arg(long, value_enum, default_value = "toon")]
    pub format: Format,
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
        /// Maximum number of matches to list (JSON defaults to 50)
        #[arg(short, long)]
        limit: Option<usize>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Get a dense architectural overview of a specific file
    Context {
        #[arg(short, long)]
        path: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Analyze the architectural impact of a symbol
    Impact {
        #[arg(short, long)]
        fqn: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        /// Maximum number of nodes to list (JSON defaults to 200)
        #[arg(short, long)]
        limit: Option<usize>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Returns current workspace context and available commands
    Status {
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        /// Depth of the project tree (defaults to 3)
        #[arg(long)]
        max_depth: Option<usize>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Extract raw code for a specific symbol using AST parsing
    Fetch {
        #[arg(short, long)]
        path: String,
        #[arg(short, long)]
        symbol: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Check the blast radius (dependencies) of a symbol
    Blast {
        #[arg(short, long)]
        path: String,
        #[arg(short, long)]
        symbol: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        /// Maximum number of dependents to list; the counts cover all (JSON defaults to 200)
        #[arg(short, long)]
        limit: Option<usize>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Semantic keyword search over the codebase index
    Search {
        query: String,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        /// Context snippet preview lines (default 3 when flag present)
        #[arg(short = 'C', long = "preview", num_args = 0..=1, default_missing_value = "3")]
        preview: Option<usize>,
        /// Maximum number of matches to list (JSON defaults to 50)
        #[arg(short, long)]
        limit: Option<usize>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Query indexed documentation (Markdown, ADRs, specs)
    Doc {
        #[arg(short, long, default_value = ".project-map", global = true)]
        index: String,
        /// Output format: toon (default, compact text) or json (for programs, costs more tokens)
        #[arg(long, value_enum, default_value = "toon", global = true)]
        format: Format,
        #[command(subcommand)]
        command: DocCommands,
    },
    /// List the operations and schemas declared in OpenAPI specs
    Api {
        /// Restrict the listing to one spec file
        path: Option<String>,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Which areas of the codebase are largest: recursive counts per directory
    Modules {
        /// Only directories under this path prefix (defaults to the whole repository)
        path_prefix: Option<String>,
        /// Directory levels below the prefix to list
        #[arg(long, default_value_t = 2)]
        max_depth: usize,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Which files use an external package, with the import line
    Imports {
        /// Package name, exact (stripe, serde_json) or by prefix with a trailing /* or .* (@sentry/*)
        package: String,
        /// Only files under this path prefix
        #[arg(long)]
        path_prefix: Option<String>,
        /// Maximum number of import lines to list; the counts cover all
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
        #[arg(short, long, default_value = ".project-map")]
        index: String,
        #[command(flatten)]
        fmt: FormatArg,
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
    Outline {
        path: String,
        /// Keep headings down to this depth (1 = top level only)
        #[arg(long)]
        max_depth: Option<usize>,
    },
    /// Read one section's raw text by path and anchor
    Section {
        path: String,
        anchor: String,
        /// Maximum characters of section text to return (defaults to 4000; JSON returns the full text unless given)
        #[arg(long)]
        max_chars: Option<usize>,
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
