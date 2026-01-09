use clap::{Parser, ValueEnum};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};

pub mod merger;

fn parse_file_size(s: &str) -> Result<u64, String> {
    let s = s.trim().to_lowercase();

    if s.ends_with("kb") {
        let num: f64 = s.trim_end_matches("kb").parse()
            .map_err(|_| format!("Invalid number in '{}'", s))?;
        Ok((num * 1024.0) as u64)
    } else if s.ends_with("mb") {
        let num: f64 = s.trim_end_matches("mb").parse()
            .map_err(|_| format!("Invalid number in '{}'", s))?;
        Ok((num * 1024.0 * 1024.0) as u64)
    } else if s.ends_with("gb") {
        let num: f64 = s.trim_end_matches("gb").parse()
            .map_err(|_| format!("Invalid number in '{}'", s))?;
        Ok((num * 1024.0 * 1024.0 * 1024.0) as u64)
    } else {
        // Assume bytes if no suffix
        s.parse()
            .map_err(|_| format!("Invalid file size '{}'. Use format like '10MB', '1GB', or '1048576'", s))
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum DedupKey {
    #[value(name = "filename-and-size")]
    FilenameAndSize,
    #[value(name = "size-only")]
    SizeOnly,
    #[value(name = "extension-and-size")]
    ExtensionAndSize,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum GroupKey {
    FilenameAndSize(String, u64),
    SizeOnly(u64),
    ExtensionAndSize(String, u64),
}

#[derive(Parser, Debug)]
#[command(name = "torrent-combine")]
struct Args {
    root_dir: PathBuf,
    #[arg(long, value_delimiter = ',')]
    src_dirs: Vec<PathBuf>,
    #[arg(long, value_parser = parse_file_size, help = "Minimum file size to process (e.g., '10MB', '1GB', '1048576'). Default: 1MB")]
    min_file_size: Option<u64>,
    #[arg(long)]
    replace: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_delimiter = ',', help = "File extensions to include (e.g., 'mkv,mp4,avi'). Default: all files")]
    extensions: Vec<String>,
    #[arg(long)]
    num_threads: Option<usize>,
    #[arg(long, value_enum, default_value = "filename-and-size")]
    dedup_mode: DedupKey,
    #[arg(long, help = "Disable memory mapping for file I/O (auto-enabled for files ≥ 5MB)")]
    no_mmap: bool,
    #[arg(long, help = "Enable verbose logging (may interfere with progress bar)")]
    verbose: bool,
}

fn collect_large_files(dirs: &[PathBuf], min_size: u64, extensions: &[String]) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut dirs_to_process: Vec<PathBuf> = dirs.iter().cloned().collect();
    let extensions: Vec<String> = extensions.iter().map(|ext| ext.to_lowercase()).collect();

    while let Some(current_dir) = dirs_to_process.pop() {
        // Validate directory exists and is accessible
        if !current_dir.exists() {
            log::warn!("Directory does not exist: {:?}", current_dir);
            continue;
        }

        if !current_dir.is_dir() {
            log::warn!("Path is not a directory: {:?}", current_dir);
            continue;
        }

        match fs::read_dir(&current_dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();

                            // Skip problematic paths
                            if let Some(path_str) = path.to_str() {
                                if path_str.contains('\0') {
                                    log::warn!("Skipping path with null bytes: {:?}", path);
                                    continue;
                                }
                            }

                            if path.is_dir() {
                                dirs_to_process.push(path);
                            } else if let Ok(metadata) = fs::metadata(&path) {
                                if metadata.len() > min_size {
                                    // Check extension filter
                                    if extensions.is_empty() || path.extension()
                                        .and_then(|ext| ext.to_str())
                                        .map(|ext| extensions.contains(&ext.to_lowercase()))
                                        .unwrap_or(false) {
                                        files.push(path);
                                    }
                                }
                            } else {
                                log::warn!("Failed to read metadata for: {:?}", path);
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to read directory entry: {:?} (error: {})", current_dir, e);
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to read directory {:?}: {}", current_dir, e);
                // Continue with other directories instead of failing completely
                continue;
            }
        }
    }

    Ok(files)
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    // Configure logging based on verbose flag
    if args.verbose {
        if std::env::var("RUST_LOG").is_err() {
            unsafe { std::env::set_var("RUST_LOG", "info") };
        }
        env_logger::Builder::from_default_env()
            .target(env_logger::Target::Stderr)
            .init();
    } else {
        if std::env::var("RUST_LOG").is_err() {
            unsafe { std::env::set_var("RUST_LOG", "warn") }; // Reduce log level to avoid interfering with progress bar
        }
        env_logger::Builder::from_default_env()
            .target(env_logger::Target::Stderr) // Send logs to stderr, progress bar uses stdout
            .init();
    }

    if args.dry_run {
        log::info!("DRY-RUN MODE: No files will be modified. Showing what would happen.");
    }

    // Validate root directory
    if !args.root_dir.exists() {
        log::error!("Root directory does not exist: {:?}", args.root_dir);
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Root directory does not exist: {:?}", args.root_dir)
        ));
    }

    if !args.root_dir.is_dir() {
        log::error!("Root path is not a directory: {:?}", args.root_dir);
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Root path is not a directory: {:?}", args.root_dir)
        ));
    }

    // Validate source directories
    for src_dir in &args.src_dirs {
        if !src_dir.exists() {
            log::warn!("Source directory does not exist: {:?}", src_dir);
        } else if !src_dir.is_dir() {
            log::warn!("Source path is not a directory: {:?}", src_dir);
        }
    }

    log::info!("Processing root directory: {:?}", args.root_dir);
    if !args.src_dirs.is_empty() {
        log::info!("Source directories: {:?}", args.src_dirs);
    }

    if let Some(num_threads) = args.num_threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build_global()
            .unwrap();
    }

    let mut all_dirs = vec![args.root_dir.clone()];
    all_dirs.extend(args.src_dirs.clone());
    let min_file_size = args.min_file_size.unwrap_or(merger::DEFAULT_MIN_FILE_SIZE);
    log::info!("Minimum file size: {} bytes ({} MB)", min_file_size, min_file_size / 1_048_576);

    // Progress bar for file discovery
    let discovery_pb = ProgressBar::new_spinner();
    discovery_pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Failed to set discovery progress bar template")
    );
    discovery_pb.set_message("Scanning for large files...");
    discovery_pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let files = collect_large_files(&all_dirs, min_file_size, &args.extensions)?;
    discovery_pb.finish_with_message("File scanning complete");
    drop(discovery_pb);

    let files_count = files.len();
    log::info!("Found {} large files", files_count);

    let mut groups: HashMap<GroupKey, Vec<PathBuf>> = HashMap::new();
    for file in files {
        // Skip files with problematic paths
        if let Some(path_str) = file.to_str() {
            if path_str.contains('\0') || path_str.len() > 4096 {
                log::warn!("Skipping file with problematic path: {:?}", file);
                continue;
            }
        }

        if let Ok(metadata) = fs::metadata(&file) {
            let size = metadata.len();
            let key = match args.dedup_mode {
                DedupKey::FilenameAndSize => {
                    if let Some(basename) =
                        file.file_name().map(|s| s.to_string_lossy().to_string())
                    {
                        // Validate filename is reasonable
                        if basename.len() > 255 {
                            log::warn!("Skipping file with very long filename: {:?}", file);
                            continue;
                        }
                        GroupKey::FilenameAndSize(basename, size)
                    } else {
                        log::warn!("Skipping file without valid filename: {:?}", file);
                        continue;
                    }
                }
                DedupKey::SizeOnly => GroupKey::SizeOnly(size),
                DedupKey::ExtensionAndSize => {
                    if let Some(extension) = file.extension().and_then(|ext| ext.to_str()) {
                        // Validate extension is reasonable
                        if extension.len() > 10 {
                            log::warn!("Skipping file with very long extension: {:?}", file);
                            continue;
                        }
                        GroupKey::ExtensionAndSize(extension.to_lowercase(), size)
                    } else {
                        log::warn!("Skipping file without valid extension: {:?}", file);
                        continue;
                    }
                }
            };
            groups.entry(key).or_insert(Vec::new()).push(file);
        } else {
            log::warn!("Failed to read metadata for file: {:?}", file);
        }
    }

    let groups_to_process: Vec<_> = groups
        .into_iter()
        .filter(|(_, paths)| paths.len() >= 2)
        .collect();
    let total_groups = groups_to_process.len();
    log::info!("Found {} groups to process", total_groups);

    // Create progress bar
    let pb = ProgressBar::new(total_groups as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .expect("Failed to set progress bar template")
            .progress_chars("#>-")
    );
    pb.set_message("Processing groups");
    pb.enable_steady_tick(std::time::Duration::from_millis(500));

    let groups_processed = Arc::new(AtomicUsize::new(0));
    let merged_groups_count = Arc::new(AtomicUsize::new(0));
    let skipped_groups_count = Arc::new(AtomicUsize::new(0));
    let pb_shared = Arc::new(pb);

    groups_to_process
        .into_par_iter()
        .for_each(|(group_key, paths)| {
            let groups_processed_cloned = Arc::clone(&groups_processed);
            let merged_groups_count_cloned = Arc::clone(&merged_groups_count);
            let skipped_groups_count_cloned = Arc::clone(&skipped_groups_count);
            let pb_cloned = Arc::clone(&pb_shared);

            let group_name = match &group_key {
                GroupKey::FilenameAndSize(basename, size) => format!("{}@{}", basename, size),
                GroupKey::SizeOnly(size) => format!("size-{}", size),
                GroupKey::ExtensionAndSize(extension, size) => format!("{}.{}", extension, size),
            };

            match merger::process_group_with_dry_run(&paths, &group_name, args.replace, &args.src_dirs, args.dry_run, args.no_mmap) {
                Ok(stats) => {
                    let processed_count = groups_processed_cloned.fetch_add(1, Ordering::SeqCst) + 1;
                    pb_cloned.set_position(processed_count as u64);

                    match stats.status {
                        merger::GroupStatus::Merged => {
                            merged_groups_count_cloned.fetch_add(1, Ordering::SeqCst);
                            let mb_per_sec = (stats.bytes_processed as f64 / 1_048_576.0)
                                / stats.processing_time.as_secs_f64();
                            // Only log at info level if verbose, otherwise debug to avoid interfering with progress bar
                            if args.verbose {
                                log::info!(
                                    "Group '{}' merged at {:.2} MB/s",
                                    group_name,
                                    mb_per_sec
                                );
                                if !stats.merged_files.is_empty() {
                                    for file in stats.merged_files {
                                        log::info!("  -> Created merged file: {}", file.display());
                                    }
                                }
                            } else {
                                log::debug!(
                                    "Group '{}' merged at {:.2} MB/s",
                                    group_name,
                                    mb_per_sec
                                );
                                if !stats.merged_files.is_empty() {
                                    for file in stats.merged_files {
                                        log::debug!("  -> Created merged file: {}", file.display());
                                    }
                                }
                            }
                        }
                        merger::GroupStatus::Skipped => {
                            skipped_groups_count_cloned.fetch_add(1, Ordering::SeqCst);
                            if args.verbose {
                                log::info!(
                                    "Group '{}' skipped (all files complete)",
                                    group_name
                                );
                            } else {
                                log::debug!(
                                    "Group '{}' skipped (all files complete)",
                                    group_name
                                );
                            }
                        }
                        merger::GroupStatus::Failed => {
                            if args.verbose {
                                log::warn!(
                                    "Group '{}' failed sanity check",
                                    group_name
                                );
                            } else {
                                log::debug!(
                                    "Group '{}' failed sanity check",
                                    group_name
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Error processing group {}: {:?}", group_name, e);
                }
            }
        });

    // Extract the progress bar from Arc to finish it
    let pb = Arc::try_unwrap(pb_shared).expect("Failed to unwrap progress bar");
    pb.finish_with_message("Processing complete");

    let final_processed = groups_processed.load(Ordering::SeqCst);
    let final_merged = merged_groups_count.load(Ordering::SeqCst);
    let final_skipped = skipped_groups_count.load(Ordering::SeqCst);

    log::info!("--------------------");
    log::info!("Processing Summary:");
    log::info!("Total groups: {}", total_groups);
    log::info!("  - Processed: {}", final_processed);
    log::info!("  - Merged: {}", final_merged);
    log::info!("  - Skipped: {}", final_skipped);
    log::info!("--------------------");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_dedup_key_enum_variants() {
        assert_eq!(
            format!("{:?}", DedupKey::FilenameAndSize),
            "FilenameAndSize"
        );
        assert_eq!(format!("{:?}", DedupKey::SizeOnly), "SizeOnly");
        assert_eq!(format!("{:?}", DedupKey::ExtensionAndSize), "ExtensionAndSize");
    }

    #[test]
    fn test_group_key_equality() {
        let key1 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key2 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key3 = GroupKey::FilenameAndSize("other.mkv".to_string(), 1024);
        let key4 = GroupKey::SizeOnly(1024);
        let key5 = GroupKey::SizeOnly(1024);
        let key6 = GroupKey::SizeOnly(2048);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_ne!(key1, key4);
        assert_eq!(key4, key5);
        assert_ne!(key4, key6);
    }

    #[test]
    fn test_group_key_hash() {
        let mut map: HashMap<GroupKey, Vec<PathBuf>> = HashMap::new();

        let key1 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key2 = GroupKey::SizeOnly(1024);

        map.insert(key1, vec![PathBuf::from("/path1")]);
        map.insert(key2, vec![PathBuf::from("/path2")]);

        assert_eq!(map.len(), 2);

        let key1_dup = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        map.entry(key1_dup)
            .or_insert(Vec::new())
            .push(PathBuf::from("/path3"));

        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_group_name_formatting_with_extension() {
        let key1 = GroupKey::ExtensionAndSize("mkv".to_string(), 2097152);
        let key2 = GroupKey::ExtensionAndSize("mp4".to_string(), 1048576);

        let name1 = match &key1 {
            GroupKey::FilenameAndSize(basename, size) => format!("{}@{}", basename, size),
            GroupKey::SizeOnly(size) => format!("size-{}", size),
            GroupKey::ExtensionAndSize(extension, size) => format!("{}.{}", extension, size),
        };

        let name2 = match &key2 {
            GroupKey::FilenameAndSize(basename, size) => format!("{}@{}", basename, size),
            GroupKey::SizeOnly(size) => format!("size-{}", size),
            GroupKey::ExtensionAndSize(extension, size) => format!("{}.{}", extension, size),
        };

        assert_eq!(name1, "mkv.2097152");
        assert_eq!(name2, "mp4.1048576");
    }

    #[test]
    fn test_cli_parsing_basic() {
        // Test the parsing logic by checking that our parse_file_size function works correctly
        assert_eq!(parse_file_size("1MB").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("10KB").unwrap(), 10_240);
    }

    #[test]
    fn test_cli_dedup_mode() {
        assert_eq!(
            format!("{:?}", DedupKey::FilenameAndSize),
            "FilenameAndSize"
        );
        assert_eq!(format!("{:?}", DedupKey::SizeOnly), "SizeOnly");
    }

    #[test]
    fn test_group_key_creation() {
        let key1 = GroupKey::FilenameAndSize("test.mkv".to_string(), 1024);
        let key2 = GroupKey::SizeOnly(1024);

        // Test that keys can be created and compared
        assert_eq!(key1, key1);
        assert_eq!(key2, key2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_parse_file_size_bytes() {
        assert_eq!(parse_file_size("1048576").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("1024").unwrap(), 1024);
        assert_eq!(parse_file_size("0").unwrap(), 0);
    }

    #[test]
    fn test_parse_file_size_kilobytes() {
        assert_eq!(parse_file_size("1KB").unwrap(), 1024);
        assert_eq!(parse_file_size("10KB").unwrap(), 10_240);
        assert_eq!(parse_file_size("1.5KB").unwrap(), 1536);
        assert_eq!(parse_file_size("100kb").unwrap(), 102_400); // case insensitive
    }

    #[test]
    fn test_parse_file_size_megabytes() {
        assert_eq!(parse_file_size("1MB").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("10MB").unwrap(), 10_485_760);
        assert_eq!(parse_file_size("0.5MB").unwrap(), 524_288);
        assert_eq!(parse_file_size("2.5mb").unwrap(), 2_621_440); // case insensitive
    }

    #[test]
    fn test_parse_file_size_gigabytes() {
        assert_eq!(parse_file_size("1GB").unwrap(), 1_073_741_824);
        assert_eq!(parse_file_size("2GB").unwrap(), 2_147_483_648);
        assert_eq!(parse_file_size("0.5GB").unwrap(), 536_870_912);
        assert_eq!(parse_file_size("1.5gb").unwrap(), 1_610_612_736); // case insensitive
    }

    #[test]
    fn test_parse_file_size_invalid() {
        assert!(parse_file_size("invalid").is_err());
        assert!(parse_file_size("10XB").is_err());
        assert!(parse_file_size("abcMB").is_err());
        assert!(parse_file_size("").is_err());
        assert!(parse_file_size("10.5.2MB").is_err());
    }

    #[test]
    fn test_parse_file_size_whitespace() {
        assert_eq!(parse_file_size(" 1MB ").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("\t10KB\n").unwrap(), 10_240);
    }
}
