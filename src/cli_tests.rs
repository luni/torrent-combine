#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::parse_file_size;
    use std::path::PathBuf;

    #[test]
    fn test_cli_parsing_basic() {
        use clap::Parser;

        let args = vec![
            "torrent-combine",
            "/test/path",
            "--min-size", "10MB",
            "--replace",
            "--dry-run",
        ];

        let parsed = Args::try_parse_from(args).unwrap();
        assert_eq!(parsed.root_dir, PathBuf::from("/test/path"));
        assert_eq!(parsed.min_file_size, Some(10 * 1024 * 1024));
        assert!(parsed.replace);
        assert!(parsed.dry_run);
    }

    #[test]
    fn test_cli_dedup_mode() {
        use clap::Parser;

        let args = vec![
            "torrent-combine",
            "/test/path",
            "--dedup", "size-only",
        ];

        let parsed = Args::try_parse_from(args).unwrap();
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
        assert_ne!(format!("{:?}", DedupKey::FilenameAndSize), format!("{:?}", DedupKey::SizeOnly));
        assert_ne!(format!("{:?}", DedupKey::SizeOnly), format!("{:?}", DedupKey::ExtensionAndSize));
        assert_ne!(format!("{:?}", DedupKey::ExtensionAndSize), format!("{:?}", DedupKey::FilenameAndSize));
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
    fn test_format_file_size() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_multiple_src_dirs_parsing() {
        use clap::Parser;

        let args = vec![
            "torrent-combine",
            "/test/path",
            "--src", "/src1,/src2,/src3",
        ];

        let parsed = Args::try_parse_from(args).unwrap();
        assert_eq!(parsed.src_dirs.len(), 3);
        assert!(parsed.src_dirs.contains(&PathBuf::from("/src1")));
        assert!(parsed.src_dirs.contains(&PathBuf::from("/src2")));
        assert!(parsed.src_dirs.contains(&PathBuf::from("/src3")));
    }

    #[test]
    fn test_multiple_exclude_parsing() {
        use clap::Parser;

        let args = vec![
            "torrent-combine",
            "/test/path",
            "--exclude", "/exclude1,/exclude2",
        ];

        let parsed = Args::try_parse_from(args).unwrap();
        assert_eq!(parsed.exclude.len(), 2);
        assert!(parsed.exclude.contains(&PathBuf::from("/exclude1")));
        assert!(parsed.exclude.contains(&PathBuf::from("/exclude2")));
    }
}
