use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// Global cleanup registry for temporary files
static TEMP_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub fn register_temp_file(path: PathBuf) {
    if let Ok(mut files) = TEMP_FILES.lock() {
        files.push(path);
    }
}

pub fn cleanup_temp_files() {
    if let Ok(files) = TEMP_FILES.lock() {
        for path in files.iter() {
            if path.exists() {
                if let Err(e) = fs::remove_file(path) {
                    log::warn!("Failed to cleanup temp file {:?}: {}", path, e);
                } else {
                    log::debug!("Cleaned up temp file: {:?}", path);
                }
            }
        }
    }
}

// Set up panic hook to cleanup on panic
pub fn setup_cleanup_on_panic() {
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("Program panicked: {}", panic_info);
        cleanup_temp_files();
    }));
}

pub fn parse_file_size(s: &str) -> Result<u64, String> {
    let s = s.trim().to_lowercase();

    if s.ends_with("kb") {
        let num: f64 = s
            .trim_end_matches("kb")
            .parse()
            .map_err(|_| format!("Invalid number in '{}'", s))?;
        Ok((num * 1024.0) as u64)
    } else if s.ends_with("mb") {
        let num: f64 = s
            .trim_end_matches("mb")
            .parse()
            .map_err(|_| format!("Invalid number in '{}'", s))?;
        Ok((num * 1024.0 * 1024.0) as u64)
    } else if s.ends_with("gb") {
        let num: f64 = s
            .trim_end_matches("gb")
            .parse()
            .map_err(|_| format!("Invalid number in '{}'", s))?;
        Ok((num * 1024.0 * 1024.0 * 1024.0) as u64)
    } else {
        // Assume bytes if no suffix
        s.parse().map_err(|_| {
            format!(
                "Invalid file size '{}'. Use format like '10MB', '1GB', or '1048576'",
                s
            )
        })
    }
}

// Atomic counter for generating unique names
static COUNTER: AtomicUsize = AtomicUsize::new(1);

pub fn get_unique_id() -> usize {
    COUNTER.fetch_add(1, Ordering::SeqCst)
}

// Helper function to format file size
pub fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}
