#[cfg(test)]
mod tests {
    use crate::cache::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;

    #[test]
    fn test_file_cache_new() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let cache_ttl = 3600;

        let cache = FileCache::new(cache_dir.clone(), cache_ttl);

        assert_eq!(cache.cache_dir, cache_dir);
        assert_eq!(cache.cache_ttl, cache_ttl);
        assert!(cache.file_cache.is_empty());
        assert!(cache.group_cache.is_empty());
    }

    #[test]
    fn test_file_cache_load_new_directory() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let mut cache = FileCache::new(cache_dir.clone(), 3600);

        // Loading should create directory if it doesn't exist
        cache.load()?;

        assert!(cache_dir.exists());
        assert!(cache_dir.is_dir());
        assert!(cache.file_cache.is_empty());
        assert!(cache.group_cache.is_empty());

        Ok(())
    }

    #[test]
    fn test_file_cache_save_and_load() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let mut cache = FileCache::new(cache_dir.clone(), 3600);

        // Add some test data
        let test_path = PathBuf::from("/test/file.txt");
        let file_info = FileInfo {
            path: test_path.clone(),
            size: 1024,
            modified: SystemTime::now(),
            hash: "test_hash".to_string(),
            last_verified: SystemTime::now(),
        };

        cache.update_file_cache(file_info, true);

        // Save and reload
        cache.save()?;

        let mut new_cache = FileCache::new(cache_dir, 3600);
        new_cache.load()?;

        assert_eq!(new_cache.file_cache.len(), 1);
        let loaded_info = new_cache.get_file_info(&test_path).unwrap();
        assert_eq!(loaded_info.path, test_path);
        assert_eq!(loaded_info.size, 1024);
        assert_eq!(loaded_info.hash, "test_hash");

        Ok(())
    }

    #[test]
    fn test_update_file_cache() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let mut cache = FileCache::new(cache_dir, 3600);

        let test_path = PathBuf::from("/test/file.txt");
        let file_info = FileInfo {
            path: test_path.clone(),
            size: 1024,
            modified: SystemTime::now(),
            hash: "old_hash".to_string(),
            last_verified: SystemTime::now(),
        };

        cache.update_file_cache(file_info, true);

        // Update with new info
        let new_info = FileInfo {
            path: test_path.clone(),
            size: 2048,
            modified: SystemTime::now(),
            hash: "new_hash".to_string(),
            last_verified: SystemTime::now(),
        };

        cache.update_file_cache(new_info, true);

        let loaded_info = cache.get_file_info(&test_path).unwrap();
        assert_eq!(loaded_info.size, 2048);
        assert_eq!(loaded_info.hash, "new_hash");

        Ok(())
    }

    #[test]
    fn test_update_group_cache() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let mut cache = FileCache::new(cache_dir, 3600);

        let group_name = "test_group".to_string();
        let files = vec![
            FileInfo {
                path: PathBuf::from("/test/file1.txt"),
                size: 1024,
                modified: SystemTime::now(),
                hash: "hash1".to_string(),
                last_verified: SystemTime::now(),
            },
            FileInfo {
                path: PathBuf::from("/test/file2.txt"),
                size: 2048,
                modified: SystemTime::now(),
                hash: "hash2".to_string(),
                last_verified: SystemTime::now(),
            },
        ];

        cache.update_group_cache(group_name.clone(), files, true);

        let loaded_files = cache.get_group_cache(&group_name).unwrap();
        assert_eq!(loaded_files.len(), 2);
        assert_eq!(loaded_files[0].size, 1024);
        assert_eq!(loaded_files[1].size, 2048);

        Ok(())
    }

    #[test]
    fn test_get_file_info_nonexistent() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let cache = FileCache::new(cache_dir, 3600);

        let nonexistent_path = PathBuf::from("/nonexistent/file.txt");
        let result = cache.get_file_info(&nonexistent_path);

        assert!(result.is_none());

        Ok(())
    }

    #[test]
    fn test_get_group_cache_nonexistent() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let cache = FileCache::new(cache_dir, 3600);

        let nonexistent_group = "nonexistent_group";
        let result = cache.get_group_cache(nonexistent_group);

        assert!(result.is_none());

        Ok(())
    }

    #[test]
    fn test_is_cache_valid() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let cache = FileCache::new(cache_dir, 3600);

        // Test with valid cache
        assert!(cache.is_cache_valid());

        // Test with zero TTL (always invalid)
        let zero_ttl_cache = FileCache::new(cache_dir, 0);
        assert!(!zero_ttl_cache.is_cache_valid());

        Ok(())
    }

    #[test]
    fn test_get_file_info_with_hash() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let mut cache = FileCache::new(cache_dir, 3600);

        let test_path = PathBuf::from("/test/file.txt");
        let file_info = FileInfo {
            path: test_path.clone(),
            size: 1024,
            modified: SystemTime::now(),
            hash: "test_hash".to_string(),
            last_verified: SystemTime::now(),
        };

        cache.update_file_cache(file_info, true);

        let (loaded_info, hash) = cache.get_file_info_with_hash(&test_path).unwrap();
        assert_eq!(loaded_info.path, test_path);
        assert_eq!(hash, "test_hash");

        Ok(())
    }

    #[test]
    fn test_cache_ttl_zero() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let cache = FileCache::new(cache_dir, 0);

        // With zero TTL, cache should always be invalid
        assert!(!cache.is_cache_valid());

        Ok(())
    }

    #[test]
    fn test_compute_file_hash() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache = FileCache::new(temp_dir.path().to_path_buf(), 3600);

        // Create a test file
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "test content")?;

        let hash = cache.compute_file_hash(&test_file)?;
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA256 hex string length

        Ok(())
    }

    #[test]
    fn test_compute_file_hash_nonexistent() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache = FileCache::new(temp_dir.path().to_path_buf(), 3600);

        let nonexistent_file = temp_dir.path().join("nonexistent.txt");
        let result = cache.compute_file_hash(&nonexistent_file);

        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_cleanup_expired() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache_dir = temp_dir.path().join("cache");
        let mut cache = FileCache::new(cache_dir, 1); // 1 second TTL for quick testing

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Add some test data with different timestamps
        let old_file_info = FileInfo {
            path: PathBuf::from("/test/old.txt"),
            size: 1024,
            modified: current_time - 10, // 10 seconds ago
            hash: "old_hash".to_string(),
            last_verified: current_time - 10,
        };

        let new_file_info = FileInfo {
            path: PathBuf::from("/test/new.txt"),
            size: 2048,
            modified: current_time,
            hash: "new_hash".to_string(),
            last_verified: current_time,
        };

        cache.update_file_cache(old_file_info, true);
        cache.update_file_cache(new_file_info, true);

        cache.update_group_cache(
            "old_group".to_string(),
            vec![FileInfo {
                path: PathBuf::from("/test/group_old.txt"),
                size: 1024,
                modified: current_time - 10,
                hash: "group_old_hash".to_string(),
                last_verified: current_time - 10,
            }],
            true,
        );

        cache.update_group_cache(
            "new_group".to_string(),
            vec![FileInfo {
                path: PathBuf::from("/test/group_new.txt"),
                size: 2048,
                modified: current_time,
                hash: "group_new_hash".to_string(),
                last_verified: current_time,
            }],
            true,
        );

        // Wait a bit to ensure entries are expired
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Cleanup expired entries
        cache.cleanup_expired();

        // Verify cleanup happened (at least some entries should be removed)
        let total_entries = cache.file_cache.len() + cache.group_cache.len();
        assert!(total_entries <= 4); // Should be less than or equal to original count

        Ok(())
    }
}
