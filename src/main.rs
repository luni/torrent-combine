use std::path::PathBuf;
use std::time::SystemTime;

use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

pub mod cache;
pub mod cli;
pub mod file_ops;
pub mod merger;
pub mod utils;

use cache::FileCache;
use cli::Args;
use utils::{cleanup_temp_files, setup_cleanup_on_panic};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Args = clap::Parser::parse();

    // Setup logging
    let log_level = if args.verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    env_logger::Builder::from_default_env()
        .filter_level(log_level)
        .init();

    // Setup cleanup on panic
    setup_cleanup_on_panic();

    // Set up thread pool
    if let Some(num_threads) = args.num_threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build_global()
            .unwrap();
    }

    // Initialize cache
    let cache_dir = args.root_dirs[0].join(".torrent-combine-cache");
    let _cache = FileCache::new(cache_dir.clone(), 3600); // 1 hour TTL

    // Clear cache if requested
    if args.clear_cache {
        // Remove cache directory
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)?;
        }
        println!("Cache cleared.");
    }

    // Determine which directories to scan and which are read-only
    let scan_dirs = if args.src_dirs.is_empty() {
        // No src dirs specified, so root_dirs are both source and target
        args.root_dirs.clone()
    } else {
        // src_dirs specified, so root_dirs are targets and src_dirs are read-only sources
        args.root_dirs.clone()
    };

    let src_dirs = args.src_dirs.clone();

    // Collect files
    println!("Scanning for files...");
    let files = file_ops::collect_large_files(
        &scan_dirs,
        args.min_file_size.unwrap_or(0),
        &args.extensions,
        &args.exclude,
    )?;

    if files.is_empty() {
        println!("No files found matching criteria.");
        return Ok(());
    }

    println!("Found {} files.", files.len());

    // Group files
    println!("Grouping files...");
    let groups = file_ops::group_files(files, &args.dedup_mode)?;

    if groups.is_empty() {
        println!("No file groups found (all files are unique).");
        return Ok(());
    }

    println!("Found {} file groups.", groups.len());

    // Process groups
    let progress = ProgressBar::new(groups.len() as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );

    let results: Vec<_> = groups
        .into_iter()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|(group_name, files)| {
            let result = process_group(group_name, files, &args, cache_dir.clone(), args.dry_run, &src_dirs);
            progress.inc(1);
            result
        })
        .collect();

    progress.finish();

    // Print summary
    let mut total_merged = 0;
    let mut total_skipped = 0;
    let mut total_failed = 0;

    for result in results {
        match result {
            Ok(stats) => {
                if !stats.merged_files.is_empty() {
                    total_merged += stats.merged_files.len();
                    println!("Merged {} files for group", stats.merged_files.len());
                } else {
                    total_skipped += 1;
                }
            }
            Err(e) => {
                total_failed += 1;
                eprintln!("Error processing group: {}", e);
            }
        }
    }

    println!("\nSummary:");
    println!("  Merged: {} files", total_merged);
    println!("  Skipped: {} groups", total_skipped);
    println!("  Failed: {} groups", total_failed);

    // Cleanup
    cleanup_temp_files();

    Ok(())
}

fn process_group(
    group_name: &str,
    files: &[PathBuf],
    args: &Args,
    cache_dir: PathBuf,
    dry_run: bool,
    src_dirs: &[PathBuf],
) -> Result<merger::GroupStats, Box<dyn std::error::Error + Send + Sync>> {
    // Initialize cache for this group
    let mut cache = FileCache::new(cache_dir, 3600);

    // Check cache first
    if !args.no_cache {
        if let Some(cached_result) = cache.get_group_cache(group_name) {
            if !cached_result.files.is_empty()
                && cached_result
                    .files
                    .iter()
                    .all(|f| files.iter().any(|file| file == &f.path))
            {
                return Ok(merger::GroupStats {
                    status: merger::GroupStatus::Skipped,
                    processing_time: std::time::Duration::from_secs(0),
                    bytes_processed: 0,
                    merged_files: vec![],
                });
            }
        }
    }

    // Process the group
    let stats = merger::process_group_with_dry_run(
        files,
        group_name,
        args.replace,
        src_dirs,
        dry_run,
        args.no_mmap,
    )?;

    // Update cache
    if !args.no_cache && !dry_run {
        let file_infos: Result<Vec<cache::FileInfo>, Box<dyn std::error::Error>> = files
            .iter()
            .map(|f| {
                let (size, modified) = file_ops::get_file_info(f)?;
                Ok(cache::FileInfo {
                    path: f.clone(),
                    size,
                    modified: modified.duration_since(std::time::UNIX_EPOCH)?.as_secs(),
                    hash: String::new(), // Will be computed later if needed
                    last_verified: SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                })
            })
            .collect();

        if let Ok(infos) = file_infos {
            cache.update_group_cache(group_name.to_string(), infos, true);
        }
    }

    Ok(stats)
}
