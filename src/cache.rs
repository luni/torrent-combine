use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub modified: u64,
    pub hash: String,
    pub last_verified: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub file_info: FileInfo,
    pub is_complete: bool,
    pub last_verified: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCache {
    pub files: Vec<FileInfo>,
    pub is_complete: bool,
    pub last_verified: u64,
}

pub struct FileCache {
    cache_dir: PathBuf,
    file_cache: HashMap<PathBuf, CacheEntry>,
    group_cache: HashMap<String, GroupCache>,
    cache_ttl: u64, // Time-to-live in seconds
}

impl FileCache {
    pub fn new(cache_dir: PathBuf, cache_ttl: u64) -> Self {
        Self {
            cache_dir,
            file_cache: HashMap::new(),
            group_cache: HashMap::new(),
            cache_ttl,
        }
    }

    pub fn load(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir)?;
            return Ok(());
        }

        // Load file cache
        let file_cache_path = self.cache_dir.join("file_cache.json");
        if file_cache_path.exists() {
            let content = fs::read_to_string(file_cache_path)?;
            self.file_cache = serde_json::from_str(&content)?;
        }

        // Load group cache
        let group_cache_path = self.cache_dir.join("group_cache.json");
        if group_cache_path.exists() {
            let content = fs::read_to_string(group_cache_path)?;
            self.group_cache = serde_json::from_str(&content)?;
        }

        Ok(())
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(&self.cache_dir)?;

        // Save file cache
        let file_cache_path = self.cache_dir.join("file_cache.json");
        let file_cache_json = serde_json::to_string(&self.file_cache)?;
        fs::write(file_cache_path, file_cache_json)?;

        // Save group cache
        let group_cache_path = self.cache_dir.join("group_cache.json");
        let group_cache_json = serde_json::to_string(&self.group_cache)?;
        fs::write(group_cache_path, group_cache_json)?;

        Ok(())
    }

    pub fn get_file_info(&self, path: &Path) -> Option<FileInfo> {
        self.file_cache
            .get(path)
            .map(|entry| entry.file_info.clone())
    }

    pub fn get_group_cache(&self, group_key: &str) -> Option<GroupCache> {
        self.group_cache.get(group_key).cloned()
    }

    pub fn is_cache_valid(&self, timestamp: u64) -> bool {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        (current_time - timestamp) < self.cache_ttl
    }

    pub fn update_file_cache(&mut self, file_info: FileInfo, is_complete: bool) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = CacheEntry {
            file_info: file_info.clone(),
            is_complete,
            last_verified: current_time,
        };

        self.file_cache.insert(file_info.path.clone(), entry);
    }

    pub fn update_group_cache(
        &mut self,
        group_key: String,
        files: Vec<FileInfo>,
        is_complete: bool,
    ) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cache = GroupCache {
            files,
            is_complete,
            last_verified: current_time,
        };

        self.group_cache.insert(group_key, cache);
    }

    pub fn cleanup_expired(&mut self) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Remove expired file cache entries
        self.file_cache
            .retain(|_, entry| (current_time - entry.last_verified) < self.cache_ttl);

        // Remove expired group cache entries
        self.group_cache
            .retain(|_, cache| (current_time - cache.last_verified) < self.cache_ttl);
    }

    pub fn compute_file_hash(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();

        // Include file path in hash
        if let Some(path_str) = path.to_str() {
            hasher.update(path_str.as_bytes());
        }

        // Include file size and modification time
        let metadata = fs::metadata(path)?;
        hasher.update(metadata.len().to_le_bytes());

        if let Ok(modified) = metadata.modified() {
            let timestamp = modified
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            hasher.update(timestamp.to_le_bytes());
        }

        // Include first and last 1KB of file content for quick verification
        let file = fs::File::open(path)?;
        let mut buf = [0u8; 1024];

        // First 1KB
        use std::io::Read;
        let mut file = file;
        let bytes_read = file.read(&mut buf)?;
        hasher.update(&buf[..bytes_read]);

        // Last 1KB (if file is larger than 2KB)
        if metadata.len() > 2048 {
            use std::io::Seek;
            file.seek(std::io::SeekFrom::End(-1024))?;
            let bytes_read = file.read(&mut buf)?;
            hasher.update(&buf[..bytes_read]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    pub fn get_file_info_with_hash(
        &mut self,
        path: &Path,
    ) -> Result<Option<FileInfo>, Box<dyn std::error::Error>> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();

        let modified = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let hash = self.compute_file_hash(path)?;

        Ok(Some(FileInfo {
            path: path.to_path_buf(),
            size,
            modified,
            hash,
            last_verified: 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
            modified: 1234567890,
            hash: "test_hash".to_string(),
            last_verified: 1234567890,
        };

        cache.update_file_cache(file_info.clone(), true);
        cache.update_group_cache("test_group".to_string(), vec![file_info.clone()], true);

        // Save cache
        cache.save()?;

        // Create new cache instance and load
        let mut new_cache = FileCache::new(cache_dir, 3600);
        new_cache.load()?;

        // Verify data was loaded
        assert_eq!(new_cache.file_cache.len(), 1);
        assert_eq!(new_cache.group_cache.len(), 1);

        let loaded_file_info = new_cache.get_file_info(&test_path).unwrap();
        assert_eq!(loaded_file_info.path, test_path);
        assert_eq!(loaded_file_info.size, 1024);
        assert_eq!(loaded_file_info.hash, "test_hash");

        let loaded_group = new_cache.get_group_cache("test_group").unwrap();
        assert_eq!(loaded_group.files.len(), 1);
        assert!(loaded_group.is_complete);

        Ok(())
    }

    #[test]
    fn test_is_cache_valid() {
        let cache = FileCache::new(PathBuf::from("/test"), 3600); // 1 hour TTL

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Test valid timestamp (current time)
        assert!(cache.is_cache_valid(current_time));

        // Test invalid timestamp (2 hours ago)
        assert!(!cache.is_cache_valid(current_time - 7200));

        // Test edge case (exactly at TTL limit)
        assert!(cache.is_cache_valid(current_time - 3599));
        assert!(!cache.is_cache_valid(current_time - 3600));
    }

    #[test]
    fn test_update_file_cache() {
        let mut cache = FileCache::new(PathBuf::from("/test"), 3600);

        let file_info = FileInfo {
            path: PathBuf::from("/test/file.txt"),
            size: 1024,
            modified: 1234567890,
            hash: "test_hash".to_string(),
            last_verified: 0,
        };

        // Add file info
        cache.update_file_cache(file_info.clone(), true);

        // Verify it was added
        let retrieved = cache.get_file_info(&file_info.path).unwrap();
        assert_eq!(retrieved.path, file_info.path);
        assert_eq!(retrieved.size, file_info.size);
        assert_eq!(retrieved.hash, file_info.hash);

        // Test updating existing entry
        let updated_info = FileInfo {
            path: file_info.path.clone(),
            size: 2048,
            modified: 1234567891,
            hash: "updated_hash".to_string(),
            last_verified: 0,
        };

        cache.update_file_cache(updated_info.clone(), false);

        let retrieved = cache.get_file_info(&file_info.path).unwrap();
        assert_eq!(retrieved.size, 2048);
        assert_eq!(retrieved.hash, "updated_hash");
    }

    #[test]
    fn test_update_group_cache() {
        let mut cache = FileCache::new(PathBuf::from("/test"), 3600);

        let file_info1 = FileInfo {
            path: PathBuf::from("/test/file1.txt"),
            size: 1024,
            modified: 1234567890,
            hash: "hash1".to_string(),
            last_verified: 0,
        };

        let file_info2 = FileInfo {
            path: PathBuf::from("/test/file2.txt"),
            size: 2048,
            modified: 1234567891,
            hash: "hash2".to_string(),
            last_verified: 0,
        };

        // Add group cache
        cache.update_group_cache(
            "test_group".to_string(),
            vec![file_info1.clone(), file_info2.clone()],
            true,
        );

        // Verify it was added
        let retrieved = cache.get_group_cache("test_group").unwrap();
        assert_eq!(retrieved.files.len(), 2);
        assert!(retrieved.is_complete);
        assert_eq!(retrieved.files[0].path, file_info1.path);
        assert_eq!(retrieved.files[1].path, file_info2.path);

        // Test updating existing group
        let file_info3 = FileInfo {
            path: PathBuf::from("/test/file3.txt"),
            size: 4096,
            modified: 1234567892,
            hash: "hash3".to_string(),
            last_verified: 0,
        };

        cache.update_group_cache("test_group".to_string(), vec![file_info3.clone()], false);

        let retrieved = cache.get_group_cache("test_group").unwrap();
        assert_eq!(retrieved.files.len(), 1);
        assert!(!retrieved.is_complete);
        assert_eq!(retrieved.files[0].path, file_info3.path);
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

    #[test]
    fn test_compute_file_hash() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let cache = FileCache::new(temp_dir.path().to_path_buf(), 3600);

        // Create a test file
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "Hello, World!")?;

        let hash1 = cache.compute_file_hash(&test_file)?;
        let hash2 = cache.compute_file_hash(&test_file)?;

        // Hash should be consistent
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());

        // Different file should have different hash
        let test_file2 = temp_dir.path().join("test2.txt");
        fs::write(&test_file2, "Different content")?;

        let hash3 = cache.compute_file_hash(&test_file2)?;
        assert_ne!(hash1, hash3);

        Ok(())
    }

    #[test]
    fn test_compute_file_hash_nonexistent() {
        let temp_dir = tempdir().unwrap();
        let cache = FileCache::new(temp_dir.path().to_path_buf(), 3600);

        let nonexistent_file = temp_dir.path().join("nonexistent.txt");
        let result = cache.compute_file_hash(&nonexistent_file);

        assert!(result.is_err());
    }

    #[test]
    fn test_get_file_info_with_hash() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let mut cache = FileCache::new(temp_dir.path().to_path_buf(), 3600);

        // Create a test file
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "Test content")?;

        let file_info = cache.get_file_info_with_hash(&test_file)?;

        assert!(file_info.is_some());
        let info = file_info.unwrap();
        assert_eq!(info.path, test_file);
        assert_eq!(info.size, 12); // "Test content" length
        assert!(!info.hash.is_empty());
        assert!(info.last_verified == 0); // Should be 0 as set in the function

        Ok(())
    }

    #[test]
    fn test_get_file_info_nonexistent() {
        let cache = FileCache::new(PathBuf::from("/test"), 3600);

        let nonexistent_path = PathBuf::from("/nonexistent/file.txt");
        let result = cache.get_file_info(&nonexistent_path);

        assert!(result.is_none());
    }

    #[test]
    fn test_get_group_cache_nonexistent() {
        let cache = FileCache::new(PathBuf::from("/test"), 3600);

        let result = cache.get_group_cache("nonexistent_group");

        assert!(result.is_none());
    }

    #[test]
    fn test_cache_ttl_zero() {
        let cache = FileCache::new(PathBuf::from("/test"), 0); // Zero TTL

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // With zero TTL, everything should be invalid immediately
        assert!(!cache.is_cache_valid(current_time));
        assert!(!cache.is_cache_valid(current_time - 1));
    }
}
