use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "fdu")]
#[command(about = "Crazy fast disk usage analyzer", long_about = None)]
#[command(version)]
pub struct Cli {
    /// Paths to analyze
    #[arg(value_name = "PATH", default_values = ["."])]
    pub paths: Vec<PathBuf>,

    /// Display all files and directories
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Display only directories
    #[arg(short = 'd', long, conflicts_with = "files_only")]
    pub dirs_only: bool,

    /// Display only files
    #[arg(short = 'f', long, conflicts_with = "dirs_only")]
    pub files_only: bool,

    /// Size display format
    #[arg(short = 'F', long, value_enum, default_value = "human")]
    pub format: SizeFormat,

    /// Display apparent size
    #[arg(long = "apparent-size")]
    pub apparent_size: bool,

    /// Block size
    #[arg(short = 'B', long = "block-size", value_name = "SIZE")]
    pub block_size: Option<String>,

    /// Produce grand total
    #[arg(short = 'c', long = "total")]
    pub total: bool,

    /// Maximum depth
    #[arg(short = 'L', long = "max-depth", value_name = "N")]
    pub max_depth: Option<usize>,

    /// Minimum depth
    #[arg(long = "min-depth", value_name = "N")]
    pub min_depth: Option<usize>,

    /// Display only a total for each path provided
    #[arg(short = 's', long = "summarize")]
    pub summarize: bool,

    /// Sort by field
    #[arg(short = 'S', long, value_enum)]
    pub sort: Option<SortField>,

    /// Reverse sort order
    #[arg(short = 'r', long)]
    pub reverse: bool,

    /// Include patterns
    #[arg(long = "include", value_name = "PATTERN")]
    pub include_patterns: Vec<String>,

    /// Exclude patterns
    #[arg(long = "exclude", value_name = "PATTERN")]
    pub exclude_patterns: Vec<String>,

    /// Exclude from file
    // pub exclude_from: Option<PathBuf>,

    /// Threshold size
    #[arg(short = 't', long = "threshold", value_name = "SIZE")]
    pub threshold: Option<String>,

    /// Count hard links
    #[arg(short = 'l', long = "count-links")]
    pub count_links: bool,

    /// Dereference (follow) symlinks
    #[arg(short = 'H', long = "dereference")]
    pub dereference: bool,

    /// Don't cross filesystem boundaries
    #[arg(short = 'x', long = "one-file-system")]
    pub one_file_system: bool,

    /// Number of threads
    #[arg(short = 'j', long = "jobs", default_value = "32")]
    pub threads: usize,

    /// Cache size in MB (for hard links)
    #[arg(long = "cache-size", default_value = "100")]
    pub cache_size_mb: usize,

    /// Disable cache for hard links
    #[arg(long = "no-cache")]
    pub no_cache: bool,

    /// Buffer errors
    #[arg(long = "buffer-errors")]
    pub buffer_errors: bool,

    /// Exclude cache directories
    #[arg(long = "exclude-caches")]
    pub exclude_caches: bool,

    /// Output format
    #[arg(short = 'o', long, value_enum)]
    pub output: Option<OutputFormat>,

    /// Show modification time
    #[arg(long = "time")]
    pub show_time: bool,

    #[arg(long = "trace", default_value = "false")]
    pub trace: bool,
}

#[derive(ValueEnum, Clone, Debug, Copy)]
pub enum SizeFormat {
    Human,
    Si,
    Blocks,
    Bytes,
    Binary,
    Hex,
    Kilo,
    Mega,
    Giga,
}

#[derive(ValueEnum, Clone, Debug, Copy)]
pub enum SortField {
    Name,
    Size,
    Count,
    Time,
}

#[derive(ValueEnum, Clone, Debug, Copy)]
pub enum OutputFormat {
    Raw,
    Json,
    // Csv,
    // Xml
}
