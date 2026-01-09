use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Minimum file size to consider (e.g., "10MB", "1GB")
    #[arg(short = 's', long = "min-size", value_parser = crate::utils::parse_file_size)]
    pub min_file_size: Option<u64>,

    /// Replace incomplete files with merged results
    #[arg(long)]
    pub replace: bool,

    /// Show what would be done without actually doing it
    #[arg(short, long)]
    pub dry_run: bool,

    /// File extensions to consider (default: all)
    #[arg(short = 'e', long = "ext")]
    pub extensions: Vec<String>,

    /// Number of threads to use (default: number of CPU cores)
    #[arg(short = 'j', long)]
    pub num_threads: Option<usize>,

    /// Deduplication mode
    #[arg(long = "dedup", default_value = "filename-and-size")]
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

    /// Source directories to search in (read-only, files won't be modified)
    #[arg(long = "src")]
    pub src_dirs: Vec<PathBuf>,

    /// Directories to exclude from search
    #[arg(long = "exclude")]
    pub exclude: Vec<PathBuf>,

    /// Root directories to search for files
    #[arg(required = true)]
    pub root_dirs: Vec<PathBuf>,
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
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                GroupKey::FilenameAndSize(filename, size)
            }
            DedupKey::SizeOnly => GroupKey::SizeOnly(size),
            DedupKey::ExtensionAndSize => {
                let extension = path
                    .extension()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_cli_parsing_basic() {
        let args = vec![
            "torrent-combine",
            "--min-size",
            "10MB",
            "--replace",
            "--dry-run",
            "/test/path",
            "/another/path",
        ];

        let parsed = Args::parse_from(args);
        assert_eq!(parsed.root_dirs.len(), 2);
        assert!(parsed.root_dirs.contains(&PathBuf::from("/test/path")));
        assert!(parsed.root_dirs.contains(&PathBuf::from("/another/path")));
        assert_eq!(parsed.min_file_size, Some(10 * 1024 * 1024));
        assert!(parsed.replace);
        assert!(parsed.dry_run);
    }

    #[test]
    fn test_cli_dedup_mode() {
        let args = vec![
            "torrent-combine",
            "--dedup",
            "size-only",
            "/test/path",
        ];

        let parsed = Args::parse_from(args);
        assert!(matches!(parsed.dedup_mode, DedupKey::SizeOnly));
    }

    #[test]
    fn test_dedup_key_enum_variants() {
        let modes = vec![
            DedupKey::FilenameAndSize,
            DedupKey::SizeOnly,
            DedupKey::ExtensionAndSize,
        ];

        for mode in modes {
            let cloned = mode.clone();
            // Note: DedupKey doesn't implement PartialEq, so we can't test equality
            let _ = cloned;

            // Test that each variant can be formatted
            let _formatted = format!("{:?}", mode);
        }

        // Test that different variants are different (by checking their debug strings)
        assert_ne!(
            format!("{:?}", DedupKey::FilenameAndSize),
            format!("{:?}", DedupKey::SizeOnly)
        );
        assert_ne!(
            format!("{:?}", DedupKey::SizeOnly),
            format!("{:?}", DedupKey::ExtensionAndSize)
        );
        assert_ne!(
            format!("{:?}", DedupKey::ExtensionAndSize),
            format!("{:?}", DedupKey::FilenameAndSize)
        );
    }

    #[test]
    fn test_group_key_creation() {
        let path = std::path::Path::new("test.mkv");
        let size = 1024;

        let filename_key = GroupKey::from_file_info(path, size, &DedupKey::FilenameAndSize);
        let size_key = GroupKey::from_file_info(path, size, &DedupKey::SizeOnly);
        let extension_key = GroupKey::from_file_info(path, size, &DedupKey::ExtensionAndSize);

        match filename_key {
            GroupKey::FilenameAndSize(name, s) => {
                assert_eq!(name, "test.mkv");
                assert_eq!(s, size);
            }
            _ => panic!("Expected FilenameAndSize"),
        }

        match size_key {
            GroupKey::SizeOnly(s) => {
                assert_eq!(s, size);
            }
            _ => panic!("Expected SizeOnly"),
        }

        match extension_key {
            GroupKey::ExtensionAndSize(ext, s) => {
                assert_eq!(ext, "mkv");
                assert_eq!(s, size);
            }
            _ => panic!("Expected ExtensionAndSize"),
        }
    }

    #[test]
    fn test_group_key_clone() {
        let key = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let cloned = key.clone();

        match (&key, &cloned) {
            (GroupKey::FilenameAndSize(name1, size1), GroupKey::FilenameAndSize(name2, size2)) => {
                assert_eq!(name1, name2);
                assert_eq!(size1, size2);
            }
            _ => panic!("Expected FilenameAndSize variants"),
        }
    }

    #[test]
    fn test_group_key_hash() {
        use std::collections::HashSet;

        let key1 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key2 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key3 = GroupKey::SizeOnly(1024);

        let mut set = HashSet::new();
        set.insert(key1);
        set.insert(key2); // Should not increase size since they're equal
        set.insert(key3);

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_group_key_partial_eq() {
        let key1 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key2 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key3 = GroupKey::SizeOnly(1024);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_ne!(key2, key3);
    }

    #[test]
    fn test_group_key_display_formatting() {
        let filename_key = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let size_key = GroupKey::SizeOnly(1024);
        let extension_key = GroupKey::ExtensionAndSize("mkv".to_string(), 1024);

        assert_eq!(format!("{}", filename_key), "test.mkv (1.0 KB)");
        assert_eq!(format!("{}", size_key), "1.0 KB");
        assert_eq!(format!("{}", extension_key), ".mkv (1.0 KB)");
    }

    #[test]
    fn test_group_name_formatting_with_extension() {
        let key = GroupKey::ExtensionAndSize("mkv".to_string(), 1024 * 1024); // 1MB
        let display = format!("{}", key);

        assert!(display.contains(".mkv"));
        assert!(display.contains("1.0 MB"));
    }

    #[test]
    fn test_multiple_src_dirs_parsing() {
        let args = vec![
            "torrent-combine",
            "--src",
            "/src1",
            "--src",
            "/src2",
            "--src",
            "/src3",
            "/test/path",
        ];

        let parsed = Args::parse_from(args);
        assert_eq!(parsed.src_dirs.len(), 3);
        assert!(parsed.src_dirs.contains(&PathBuf::from("/src1")));
        assert!(parsed.src_dirs.contains(&PathBuf::from("/src2")));
        assert!(parsed.src_dirs.contains(&PathBuf::from("/src3")));
    }

    #[test]
    fn test_multiple_exclude_parsing() {
        let args = vec![
            "torrent-combine",
            "--exclude",
            "/exclude1",
            "--exclude",
            "/exclude2",
            "/test/path",
        ];

        let parsed = Args::parse_from(args);
        assert_eq!(parsed.exclude.len(), 2);
        assert!(parsed.exclude.contains(&PathBuf::from("/exclude1")));
        assert!(parsed.exclude.contains(&PathBuf::from("/exclude2")));
    }
}
