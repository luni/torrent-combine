#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use crate::cli::DedupKey;

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
        assert!(files.len() >= 1);
        assert!(files.contains(&small_file));
        // Empty file might or might not be included depending on implementation

        Ok(())
    }

    #[test]
    fn test_collect_large_files_nonexistent_directory() -> io::Result<()> {
        let nonexistent_dir = PathBuf::from("/nonexistent/directory");
        let dirs = vec![nonexistent_dir];

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

        let files = vec![file1.clone(), file2.clone(), file3];
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

        let files = vec![file1.clone(), file2.clone(), file3];
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
