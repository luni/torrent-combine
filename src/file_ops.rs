use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cli::DedupKey;
use crate::utils::get_unique_id;

/// Collect large files from the given directories
pub fn collect_large_files(
    dirs: &[PathBuf],
    min_size: u64,
    extensions: &[String],
    exclude_dirs: &[PathBuf],
) -> io::Result<Vec<PathBuf>> {
    let mut all_files = Vec::new();

    for dir in dirs {
        match collect_files_from_dir(dir, min_size, extensions, exclude_dirs) {
            Ok(mut files) => {
                all_files.append(&mut files);
            }
            Err(e) => {
                log::warn!("Failed to read directory {:?}: {}", dir, e);
                continue;
            }
        }
    }

    Ok(all_files)
}

fn collect_files_from_dir(
    dir: &Path,
    min_size: u64,
    extensions: &[String],
    exclude_dirs: &[PathBuf],
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    // Check if this directory should be excluded
    if exclude_dirs.iter().any(|exclude| {
        dir.starts_with(exclude) || dir == exclude.as_path()
    }) {
        return Ok(files);
    }

    let entries = fs::read_dir(dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recursively collect from subdirectories
            match collect_files_from_dir(&path, min_size, extensions, exclude_dirs) {
                Ok(sub_files) => files.extend(sub_files),
                Err(e) => {
                    log::warn!("Failed to read directory {:?}: {}", path, e);
                    continue;
                }
            }
        } else if path.is_file() {
            // Check file size and extension
            if let Ok(metadata) = fs::metadata(&path) {
                let file_size = metadata.len();

                if file_size >= min_size {
                    // Check extension filter
                    let extension_match = extensions.is_empty() ||
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
                            .unwrap_or(false);

                    if extension_match {
                        files.push(path);
                    }
                }
            }
        }
    }

    Ok(files)
}

/// Group files by the specified deduplication key
pub fn group_files(files: Vec<PathBuf>, dedup_mode: &DedupKey) -> io::Result<HashMap<String, Vec<PathBuf>>> {
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for file_path in files {
        let metadata = fs::metadata(&file_path)?;
        let size = metadata.len();

        let _group_key = crate::cli::GroupKey::from_file_info(&file_path, size, dedup_mode);
        let group_name = format!("group_{}", get_unique_id());

        groups.entry(group_name).or_default().push(file_path);
    }

    // Filter out groups with only one file
    groups.retain(|_, files| files.len() > 1);

    Ok(groups)
}

/// Get file information for caching
pub fn get_file_info(path: &Path) -> io::Result<(u64, std::time::SystemTime)> {
    let metadata = fs::metadata(path)?;
    let size = metadata.len();
    let modified = metadata.modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    Ok((size, modified))
}
