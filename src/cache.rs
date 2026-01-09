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
