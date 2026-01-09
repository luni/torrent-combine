use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Root directory to search for files
    #[arg(short, long)]
    pub root_dir: PathBuf,

    /// Source directories to search in (default: root_dir)
    #[arg(long = "src", value_parser, value_delimiter = ',')]
    pub src_dirs: Vec<PathBuf>,

    /// Directories to exclude from search
    #[arg(long = "exclude", value_parser, value_delimiter = ',')]
    pub exclude: Vec<PathBuf>,

    /// Minimum file size to consider (e.g., "10MB", "1GB")
    #[arg(short = 's', long = "min-size", value_parser = crate::utils::parse_file_size)]
    pub min_file_size: Option<u64>,

    /// Replace incomplete files with merged results
    #[arg(short, long)]
    pub replace: bool,

    /// Show what would be done without actually doing it
    #[arg(short, long)]
    pub dry_run: bool,

    /// File extensions to consider (default: all)
    #[arg(short = 'e', long = "ext", value_parser, value_delimiter = ',')]
    pub extensions: Vec<String>,

    /// Number of threads to use (default: number of CPU cores)
    #[arg(short = 'j', long)]
    pub num_threads: Option<usize>,

    /// Deduplication mode
    #[arg(long = "dedup", default_value = "filename-size")]
    pub dedup_mode: DedupKey,

    /// Disable memory-mapped I/O
    #[arg(long)]
    pub no_mmap: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Disable caching
    #[arg(long)]
    pub no_cache: bool,

    /// Clear cache before processing
    #[arg(long)]
    pub clear_cache: bool,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum DedupKey {
    FilenameAndSize,
    SizeOnly,
    ExtensionAndSize,
}

// Group key for deduplication
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum GroupKey {
    FilenameAndSize(String, u64),
    SizeOnly(u64),
    ExtensionAndSize(String, u64),
}

impl GroupKey {
    pub fn from_file_info(path: &std::path::Path, size: u64, mode: &DedupKey) -> Self {
        match mode {
            DedupKey::FilenameAndSize => {
                let filename = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                GroupKey::FilenameAndSize(filename, size)
            }
            DedupKey::SizeOnly => GroupKey::SizeOnly(size),
            DedupKey::ExtensionAndSize => {
                let extension = path.extension()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                GroupKey::ExtensionAndSize(extension, size)
            }
        }
    }
}

impl std::fmt::Display for GroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroupKey::FilenameAndSize(name, size) => {
                write!(f, "{} ({})", name, crate::utils::format_file_size(*size))
            }
            GroupKey::SizeOnly(size) => {
                write!(f, "{}", crate::utils::format_file_size(*size))
            }
            GroupKey::ExtensionAndSize(ext, size) => {
                write!(f, ".{} ({})", ext, crate::utils::format_file_size(*size))
            }
        }
    }
}
