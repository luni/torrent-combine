#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_file_size_bytes() {
        assert_eq!(parse_file_size("1024").unwrap(), 1024);
        assert_eq!(parse_file_size("0").unwrap(), 0);
        assert_eq!(parse_file_size("1048576").unwrap(), 1048576);
    }

    #[test]
    fn test_parse_file_size_kilobytes() {
        assert_eq!(parse_file_size("1KB").unwrap(), 1024);
        assert_eq!(parse_file_size("10KB").unwrap(), 10240);
        assert_eq!(parse_file_size("1.5KB").unwrap(), 1536);
    }

    #[test]
    fn test_parse_file_size_megabytes() {
        assert_eq!(parse_file_size("1MB").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("10MB").unwrap(), 10_485_760);
        assert_eq!(parse_file_size("2.5MB").unwrap(), 2_621_440);
    }

    #[test]
    fn test_parse_file_size_gigabytes() {
        assert_eq!(parse_file_size("1GB").unwrap(), 1_073_741_824);
        assert_eq!(parse_file_size("2GB").unwrap(), 2_147_483_648);
    }

    #[test]
    fn test_parse_file_size_case_insensitive() {
        assert_eq!(parse_file_size("1kb").unwrap(), 1024);
        assert_eq!(parse_file_size("1KB").unwrap(), 1024);
        assert_eq!(parse_file_size("1mb").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("1MB").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("1gb").unwrap(), 1_073_741_824);
        assert_eq!(parse_file_size("1GB").unwrap(), 1_073_741_824);
    }

    #[test]
    fn test_parse_file_size_whitespace() {
        assert_eq!(parse_file_size(" 1 MB ").unwrap(), 1_048_576);
        assert_eq!(parse_file_size("\t2GB\n").unwrap(), 2_147_483_648);
        assert_eq!(parse_file_size(" 10KB ").unwrap(), 10240);
    }

    #[test]
    fn test_parse_file_size_invalid() {
        assert!(parse_file_size("").is_err());
        assert!(parse_file_size("abc").is_err());
        assert!(parse_file_size("1XB").is_err());
        assert!(parse_file_size("1.5.5MB").is_err());
        assert!(parse_file_size("1MBB").is_err());
        assert!(parse_file_size("1TB").is_err()); // Not supported
    }

    #[test]
    fn test_get_unique_id() {
        let id1 = get_unique_id();
        let id2 = get_unique_id();
        let id3 = get_unique_id();

        assert!(id1 < id2);
        assert!(id2 < id3);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    #[test]
    fn test_temp_file_registry() {
        // Test that temp file registration doesn't panic
        let test_path = PathBuf::from("/tmp/test_cleanup.tmp");
        register_temp_file(test_path);

        // Test that cleanup doesn't panic
        cleanup_temp_files();
    }

    #[test]
    fn test_setup_cleanup_on_panic() {
        // Test that setup doesn't panic
        setup_cleanup_on_panic();
    }
}
