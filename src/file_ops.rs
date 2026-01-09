use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cli::DedupKey;

/// Collect large files from the given directories
pub fn collect_large_files(
    dirs: &[PathBuf],
    min_size: u64,
    extensions: &[String],
    exclude_dirs: &[PathBuf],
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    // Check if this directory should be excluded
    if exclude_dirs.iter().any(|exclude| {
        dirs.iter()
            .any(|dir| dir.starts_with(exclude) || dir == exclude.as_path())
    }) {
        return Ok(files);
    }

    for dir in dirs {
        match fs::read_dir(dir) {
            Ok(entries) => {
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
                                let extension_match = extensions.is_empty()
                                    || path
                                        .extension()
                                        .and_then(|ext| ext.to_str())
                                        .map(|ext| {
                                            extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
                                        })
                                        .unwrap_or(false);

                                if extension_match {
                                    files.push(path);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to read directory {:?}: {}", dir, e);
                continue;
            }
        }
    }

    Ok(files)
}

fn collect_files_from_dir(
    dir: &Path,
    min_size: u64,
    extensions: &[String],
    exclude_dirs: &[PathBuf],
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    // Check if this directory should be excluded
    if exclude_dirs
        .iter()
        .any(|exclude| dir.starts_with(exclude) || dir == exclude.as_path())
    {
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
                    let extension_match = extensions.is_empty()
                        || path
                            .extension()
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
pub fn group_files(
    files: Vec<PathBuf>,
    dedup_mode: &DedupKey,
) -> io::Result<HashMap<String, Vec<PathBuf>>> {
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for file_path in files {
        let metadata = fs::metadata(&file_path)?;
        let size = metadata.len();

        let group_key = crate::cli::GroupKey::from_file_info(&file_path, size, dedup_mode);
        let group_name = format!("{:?}", group_key); // Use debug string as group key

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
    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    Ok((size, modified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::DedupKey;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_collect_large_files_empty_directory() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let dirs = vec![temp_dir.path().to_path_buf()];

        let files = collect_large_files(&dirs, 1, &[], &[])?;
        assert_eq!(files.len(), 0);

        Ok(())
    }

    #[test]
    fn test_collect_large_files_with_min_size() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create test files
        let small_file = base_path.join("small.txt");
        let large_file = base_path.join("large.txt");

        fs::write(&small_file, "small")?; // 5 bytes
        fs::write(&large_file, vec![0u8; 2048])?; // 2KB

        let dirs = vec![base_path.to_path_buf()];
        let files = collect_large_files(&dirs, 1024, &[], &[])?; // 1KB minimum

        assert_eq!(files.len(), 1);
        assert!(files.contains(&large_file));
        assert!(!files.contains(&small_file));

        Ok(())
    }

    #[test]
    fn test_collect_large_files_with_extension_filter() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create test files
        let mkv_file = base_path.join("test.mkv");
        let mp4_file = base_path.join("test.mp4");
        let txt_file = base_path.join("test.txt");

        fs::write(&mkv_file, "mkv content")?;
        fs::write(&mp4_file, "mp4 content")?;
        fs::write(&txt_file, "txt content")?;

        let dirs = vec![base_path.to_path_buf()];
        let extensions = vec!["mkv".to_string(), "mp4".to_string()];
        let files = collect_large_files(&dirs, 1, &extensions, &[])?;

        assert_eq!(files.len(), 2);
        assert!(files.contains(&mkv_file));
        assert!(files.contains(&mp4_file));
        assert!(!files.contains(&txt_file));

        Ok(())
    }

    #[test]
    fn test_collect_large_files_with_case_insensitive_extensions() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create files with mixed case extensions
        let mkv_file = base_path.join("test.mkv");
        let mkv_file_upper = base_path.join("test2.MKV");
        let mp4_file = base_path.join("test.mp4");

        fs::write(&mkv_file, "mkv content")?;
        fs::write(&mkv_file_upper, "MKV content")?;
        fs::write(&mp4_file, "mp4 content")?;

        let dirs = vec![base_path.to_path_buf()];
        let extensions = vec!["mkv".to_string()]; // lowercase
        let files = collect_large_files(&dirs, 1, &extensions, &[])?;

        // Should find both mkv files (case insensitive)
        assert_eq!(files.len(), 2);
        assert!(files.contains(&mkv_file));
        assert!(files.contains(&mkv_file_upper));
        assert!(!files.contains(&mp4_file));

        Ok(())
    }

    #[test]
    fn test_collect_large_files_with_nested_directories() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create nested directory structure
        let sub_dir = base_path.join("subdir");
        fs::create_dir(&sub_dir)?;

        // Create files in both directories
        let root_file = base_path.join("root.txt");
        let sub_file = sub_dir.join("sub.txt");

        fs::write(&root_file, vec![0u8; 1024])?;
        fs::write(&sub_file, vec![0u8; 1024])?;

        let dirs = vec![base_path.to_path_buf()];
        let files = collect_large_files(&dirs, 1, &[], &[])?;

        assert_eq!(files.len(), 2);
        assert!(files.contains(&root_file));
        assert!(files.contains(&sub_file));

        Ok(())
    }

    #[test]
    fn test_collect_large_files_with_excludes() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create directory structure
        let include_dir = base_path.join("include");
        let exclude_dir = base_path.join("exclude");
        fs::create_dir(&include_dir)?;
        fs::create_dir(&exclude_dir)?;

        // Create files
        let include_file = include_dir.join("include.txt");
        let exclude_file = exclude_dir.join("exclude.txt");

        fs::write(&include_file, vec![0u8; 1024])?;
        fs::write(&exclude_file, vec![0u8; 1024])?;

        let dirs = vec![base_path.to_path_buf()];
        let exclude_dirs = vec![exclude_dir.clone()];
        let files = collect_large_files(&dirs, 1, &[], &exclude_dirs)?;

        assert_eq!(files.len(), 1);
        assert!(files.contains(&include_file));
        assert!(!files.contains(&exclude_file));

        Ok(())
    }

    #[test]
    fn test_collect_large_files_with_zero_min_size() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create test files of different sizes
        let empty_file = base_path.join("empty.txt");
        let small_file = base_path.join("small.txt");

        fs::write(&empty_file, "")?;
        fs::write(&small_file, "x")?;

        let dirs = vec![base_path.to_path_buf()];
        let files = collect_large_files(&dirs, 0, &[], &[])?;

        // Should find both files since min_size is 0, but empty files might be filtered out
        assert!(!files.is_empty());
        assert!(files.contains(&small_file));
        // Empty file might or might not be included depending on implementation

        Ok(())
    }

    #[test]
    fn test_collect_large_files_nonexistent_directory() -> io::Result<()> {
        let nonexistent_dir = PathBuf::from("/nonexistent/directory");
        let dirs = vec![nonexistent_dir];

        // Should handle nonexistent directories gracefully
        let files = collect_large_files(&dirs, 1, &[], &[])?;
        assert_eq!(files.len(), 0);

        Ok(())
    }

    #[test]
    fn test_group_files_filename_and_size() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create files with same name and size
        let file1 = base_path.join("test.mkv");
        let file2 = base_path.join("test2.mkv"); // Different name, same size
        let file3 = base_path.join("other.txt"); // Different name and size

        fs::write(&file1, vec![0u8; 1024])?;
        fs::write(&file2, vec![0u8; 1024])?;
        fs::write(&file3, vec![0u8; 2048])?;

        let files = vec![file1.clone(), file2.clone(), file3];
        let groups = group_files(files, &DedupKey::FilenameAndSize)?;

        // Should have one group with file1 and file2 (same size, but different names won't group)
        // Actually, with FilenameAndSize, they won't group since names are different
        assert_eq!(groups.len(), 0); // No groups since all have different names

        Ok(())
    }

    #[test]
    fn test_group_files_size_only() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create files with same size
        let file1 = base_path.join("test.mkv");
        let file2 = base_path.join("test2.mp4");
        let file3 = base_path.join("other.txt");

        fs::write(&file1, vec![0u8; 1024])?;
        fs::write(&file2, vec![0u8; 1024])?;
        fs::write(&file3, vec![0u8; 2048])?;

        let files = vec![file1.clone(), file2.clone(), file3.clone()];
        let groups = group_files(files, &DedupKey::SizeOnly)?;

        // Should have one group with file1 and file2 (same size)
        assert_eq!(groups.len(), 1);

        let group_files = groups.values().next().unwrap();
        assert_eq!(group_files.len(), 2);
        assert!(group_files.contains(&file1));
        assert!(group_files.contains(&file2));
        assert!(!group_files.contains(&file3));

        Ok(())
    }

    #[test]
    fn test_group_files_extension_and_size() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create files with same extension and size
        let file1 = base_path.join("test1.mkv");
        let file2 = base_path.join("test2.mkv");
        let file3 = base_path.join("other.mp4");

        fs::write(&file1, vec![0u8; 1024])?;
        fs::write(&file2, vec![0u8; 1024])?;
        fs::write(&file3, vec![0u8; 1024])?;

        let files = vec![file1.clone(), file2.clone(), file3.clone()];
        let groups = group_files(files, &DedupKey::ExtensionAndSize)?;

        // Should have one group with file1 and file2 (same extension and size)
        assert_eq!(groups.len(), 1);

        let group_files = groups.values().next().unwrap();
        assert_eq!(group_files.len(), 2);
        assert!(group_files.contains(&file1));
        assert!(group_files.contains(&file2));
        assert!(!group_files.contains(&file3));

        Ok(())
    }

    #[test]
    fn test_get_file_info() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        let test_file = base_path.join("test.txt");
        let content = b"test content";
        fs::write(&test_file, content)?;

        let (size, modified) = get_file_info(&test_file)?;

        assert_eq!(size, content.len() as u64);
        assert!(modified <= std::time::SystemTime::now());

        Ok(())
    }

    #[test]
    fn test_group_files_no_duplicates() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create files with different sizes
        let file1 = base_path.join("test1.mkv");
        let file2 = base_path.join("test2.mp4");
        let file3 = base_path.join("other.txt");

        fs::write(&file1, vec![0u8; 1024])?;
        fs::write(&file2, vec![0u8; 2048])?;
        fs::write(&file3, vec![0u8; 4096])?;

        let files = vec![file1, file2, file3];
        let groups = group_files(files, &DedupKey::SizeOnly)?;

        // Should have no groups since all files have different sizes
        assert_eq!(groups.len(), 0);

        Ok(())
    }
}
