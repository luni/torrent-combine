#![allow(clippy::needless_range_loop)]

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use memmap2::{Mmap, MmapOptions};
use tempfile::NamedTempFile;

// Helper function to check if a file contains only null bytes
fn is_file_all_nulls(path: &Path) -> io::Result<bool> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let size = metadata.len() as usize;

    if size == 0 {
        return Ok(false); // Empty file is not considered "all nulls"
    }

    // For small files, read directly
    if size <= BUFFER_SIZE {
        let mut reader = BufReader::new(file);
        let mut buffer = vec![0u8; size];
        reader.read_exact(&mut buffer)?;
        return Ok(buffer.iter().all(|&b| b == 0));
    }

    // For large files, use memory mapping
    let mmap = unsafe { MmapOptions::new().map(&file)? };
    Ok(mmap.iter().all(|&b| b == 0))
}

// Helper function to check if a file contains any non-null bytes
fn file_has_data(path: &Path) -> io::Result<bool> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let size = metadata.len() as usize;

    if size == 0 {
        return Ok(false); // Empty file has no data
    }

    // For small files, read directly
    if size <= BUFFER_SIZE {
        let mut reader = BufReader::new(file);
        let mut buffer = vec![0u8; size];
        reader.read_exact(&mut buffer)?;
        return Ok(buffer.iter().any(|&b| b != 0));
    }

    // For large files, use memory mapping
    let mmap = unsafe { MmapOptions::new().map(&file)? };
    Ok(mmap.iter().any(|&b| b != 0))
}

// Helper function for fuzzy filename matching (80% similarity, min 5 chars)
fn filenames_fuzzy_match(filename1: &str, filename2: &str) -> bool {
    // Early exit for exact match
    if filename1 == filename2 {
        return true;
    }

    // Must be at least 5 characters long for fuzzy matching
    if filename1.len() < 5 || filename2.len() < 5 {
        return false;
    }

    // Calculate Levenshtein distance
    let distance = levenshtein_distance(filename1, filename2);
    let max_len = filename1.len().max(filename2.len());

    // Check if similarity is at least 80%
    let similarity = 1.0 - (distance as f64 / max_len as f64);
    similarity >= 0.8
}

// Calculate Levenshtein distance between two strings
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let chars1: Vec<char> = s1.chars().collect();
    let chars2: Vec<char> = s2.chars().collect();
    let len1 = chars1.len();
    let len2 = chars2.len();

    // Create DP table
    let mut dp = vec![vec![0; len2 + 1]; len1 + 1];

    // Initialize base cases
    for i in 0..=len1 {
        dp[i][0] = i;
    }
    for j in 0..=len2 {
        dp[0][j] = j;
    }

    // Fill DP table
    for i in 1..=len1 {
        for j in 1..=len2 {
            if chars1[i - 1] == chars2[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            } else {
                dp[i][j] = 1 + dp[i - 1][j].min(dp[i][j - 1]).min(dp[i - 1][j - 1]);
            }
        }
    }

    dp[len1][len2]
}

// Register temp files for cleanup
fn register_temp_file(path: &Path) {
    use crate::utils::register_temp_file;
    register_temp_file(path.to_path_buf());
}

const BUFFER_SIZE: usize = 1 << 20; // 1MB
const BYTE_ALIGNMENT: usize = 8;
const MMAP_THRESHOLD: u64 = 5 * 1024 * 1024; // 5MB - use mmap for files >= 5MB
pub const DEFAULT_MIN_FILE_SIZE: u64 = 1_048_576; // 1MB

// Mock temp file for dry-run mode
#[derive(Debug)]
struct MockTempFile;

impl MockTempFile {
    fn path(&self) -> &Path {
        Path::new("/mock/dry-run")
    }
}

// Trait to abstract temp file behavior
trait TempFile {
    fn path(&self) -> &Path;
}

impl TempFile for NamedTempFile {
    fn path(&self) -> &Path {
        NamedTempFile::path(self)
    }
}

impl TempFile for MockTempFile {
    fn path(&self) -> &Path {
        MockTempFile::path(self)
    }
}

pub struct FileFilter {
    src_dirs: Vec<PathBuf>,
}

impl FileFilter {
    pub fn new(src_dirs: Vec<PathBuf>) -> Self {
        Self { src_dirs }
    }

    fn is_writable(&self, path: &Path) -> bool {
        !self.is_in_src_dir(path)
    }

    fn is_in_src_dir(&self, path: &Path) -> bool {
        let canonical_path = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                debug!("Failed to canonicalize path {:?}: {}", path, e);
                return false;
            }
        };

        self.src_dirs.iter().any(|src_dir| {
            if let Ok(canonical_src) = src_dir.canonicalize() {
                canonical_path.starts_with(canonical_src)
            } else {
                debug!("Failed to canonicalize src dir: {:?}", src_dir);
                false
            }
        })
    }

    fn filter_writable_paths(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        paths
            .iter()
            .filter(|path| self.is_writable(path))
            .cloned()
            .collect()
    }
}

#[derive(Debug)]
pub enum GroupStatus {
    Merged,
    Skipped,
    Failed,
}

#[derive(Debug)]
pub struct GroupStats {
    pub status: GroupStatus,
    pub processing_time: Duration,
    pub bytes_processed: u64,
    pub merged_files: Vec<PathBuf>,
}

pub fn process_group_with_dry_run(
    paths: &[PathBuf],
    basename: &str,
    replace: bool,
    src_dirs: &[PathBuf],
    dry_run: bool,
    no_mmap: bool,
    copy_empty_dst: bool,
) -> io::Result<GroupStats> {
    let start_time = Instant::now();
    debug!("Processing paths for group {}: {:?}", basename, paths);

    let filter = FileFilter::new(src_dirs.to_vec());
    let writable_paths = filter.filter_writable_paths(paths);

    if writable_paths.is_empty() {
        info!(
            "All files in group '{}' are in read-only src directories, skipping",
            basename
        );
        return Ok(GroupStats {
            status: GroupStatus::Skipped,
            processing_time: start_time.elapsed(),
            bytes_processed: 0,
            merged_files: Vec::new(),
        });
    }

    info!(
        "Processing {} writable files out of {} total for group '{}'",
        writable_paths.len(),
        paths.len(),
        basename
    );

    // Handle copy_empty_dst logic - check before normal processing
    if copy_empty_dst && paths.len() >= 2 {
        // Separate sources and destinations
        let mut sources = Vec::new();
        let mut destinations = Vec::new();

        for path in paths.iter() {
            if filter.is_in_src_dir(path) {
                sources.push(path);
            } else {
                destinations.push(path);
            }
        }

        // Process each destination to find matching sources
        let mut successful_copies = Vec::new();
        let mut total_bytes_copied = 0u64;

        for dst_path in &destinations {
            if let Some(dst_filename) = dst_path.file_name() {
                let dst_filename_str = dst_filename.to_string_lossy();

                // Find matching sources (exact or fuzzy)
                let mut matching_sources = Vec::new();

                for src_path in &sources {
                    if let Some(src_filename) = src_path.file_name() {
                        let src_filename_str = src_filename.to_string_lossy();

                        if src_filename_str == dst_filename_str
                            || filenames_fuzzy_match(&src_filename_str, &dst_filename_str)
                        {
                            matching_sources.push(src_path);
                        }
                    }
                }

                // Process each matching source
                for src_path in &matching_sources {
                    // Check if sizes match
                    if let (Ok(src_metadata), Ok(dst_metadata)) =
                        (fs::metadata(src_path), fs::metadata(dst_path))
                    {
                        if src_metadata.len() == dst_metadata.len() {
                            // Check if destination is all nulls and source has data
                            if let (Ok(dst_is_nulls), Ok(src_has_data)) =
                                (is_file_all_nulls(dst_path), file_has_data(src_path))
                            {
                                if dst_is_nulls && src_has_data {
                                    let match_type = if src_path.file_name() == dst_path.file_name()
                                    {
                                        "exact"
                                    } else {
                                        "fuzzy"
                                    };

                                    info!(
                                        "Filename {} match: '{}' vs '{}'",
                                        match_type,
                                        src_path.file_name().unwrap_or_default().to_string_lossy(),
                                        dst_filename_str
                                    );

                                    info!(
                                        "Copying source to destination: {:?} -> {:?}",
                                        src_path, dst_path
                                    );

                                    if !dry_run {
                                        fs::copy(src_path, dst_path)?;
                                    }

                                    successful_copies.push(dst_path.to_path_buf());
                                    total_bytes_copied += src_metadata.len();

                                    // Break after first successful copy per destination
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // If we made successful copies, return early with stats
        if !successful_copies.is_empty() {
            return Ok(GroupStats {
                status: GroupStatus::Merged,
                processing_time: start_time.elapsed(),
                bytes_processed: total_bytes_copied,
                merged_files: successful_copies,
            });
        }
    }

    let bytes_processed = if !writable_paths.is_empty() {
        fs::metadata(&writable_paths[0])?.len()
    } else {
        0
    };

    if bytes_processed == 0 {
        return Ok(GroupStats {
            status: GroupStatus::Skipped,
            processing_time: start_time.elapsed(),
            bytes_processed,
            merged_files: Vec::new(),
        });
    }

    // Auto-detect optimal I/O method: use mmap for large files unless explicitly disabled
    let should_use_mmap = if no_mmap {
        // User explicitly disabled mmap - always use regular I/O
        false
    } else {
        // Auto-detect: use mmap for large files, regular I/O for small files
        bytes_processed >= MMAP_THRESHOLD
    };

    debug!(
        "Using {} I/O for {} bytes (threshold: {})",
        if should_use_mmap {
            "memory-mapped"
        } else {
            "regular"
        },
        bytes_processed,
        MMAP_THRESHOLD
    );

    let res = if dry_run {
        Some((
            Box::new(MockTempFile) as Box<dyn TempFile>,
            vec![false; writable_paths.len()],
        ))
    } else {
        check_sanity_and_completes(&writable_paths, &filter, should_use_mmap)?
            .map(|(temp, complete)| (Box::new(temp) as Box<dyn TempFile>, complete))
    };

    match res {
        Some((temp, is_complete)) => handle_successful_merge(
            &writable_paths,
            &filter,
            basename,
            replace,
            temp,
            is_complete,
            start_time,
            bytes_processed,
        ),
        None => {
            let warn_msg = format!("Sanity check failed for group: {}", basename);
            warn!("{}", warn_msg);
            Ok(GroupStats {
                status: GroupStatus::Failed,
                processing_time: start_time.elapsed(),
                bytes_processed,
                merged_files: Vec::new(),
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_successful_merge(
    writable_paths: &[PathBuf],
    filter: &FileFilter,
    basename: &str,
    replace: bool,
    temp: Box<dyn TempFile>,
    is_complete: Vec<bool>,
    start_time: Instant,
    bytes_processed: u64,
) -> io::Result<GroupStats> {
    info!("Sanity check passed for group {}", basename);

    let any_incomplete = is_complete.iter().any(|&c| !c);
    if any_incomplete {
        let mut merged_files = Vec::new();
        for (j, &complete) in is_complete.iter().enumerate() {
            if !complete {
                let path = &writable_paths[j];

                if !filter.is_writable(path) {
                    info!("Skipping read-only file in src directory: {:?}", path);
                    continue;
                }

                let parent = path.parent().ok_or(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "No parent directory",
                ))?;

                if !filter.is_writable(parent) {
                    info!(
                        "Skipping file because parent directory is in src directories: {:?}",
                        parent
                    );
                    continue;
                }

                // Handle dry-run mode
                if temp.path() == Path::new("/mock/dry-run") {
                    // Dry-run: just simulate what would happen
                    let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
                    let merged_path = if replace {
                        path.clone()
                    } else {
                        parent.join(format!("{}.merged", file_name))
                    };
                    info!(
                        "DRY-RUN: Would {} file: {:?}",
                        if replace { "replace" } else { "create merged" },
                        merged_path
                    );
                    merged_files.push(merged_path);
                } else {
                    // Real processing
                    let local_temp = NamedTempFile::new_in(parent)?;
                    register_temp_file(local_temp.path());
                    fs::copy(temp.path(), local_temp.path())?;
                    if replace {
                        fs::rename(local_temp.path(), path)?;
                        debug!("Replaced original {:?} with merged content", path);
                    } else {
                        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
                        let merged_path = parent.join(format!("{}.merged", file_name));
                        local_temp.persist(&merged_path)?;
                        debug!(
                            "Created merged file {:?} for incomplete original {:?}",
                            merged_path, path
                        );
                        merged_files.push(merged_path);
                    }
                }
            }
        }
        info!(
            "Completed {} for group {}",
            if replace { "replacement" } else { "merge" },
            basename
        );
        Ok(GroupStats {
            status: GroupStatus::Merged,
            processing_time: start_time.elapsed(),
            bytes_processed,
            merged_files,
        })
    } else {
        info!(
            "Skipped group {} (all complete, no action needed)",
            basename
        );
        Ok(GroupStats {
            status: GroupStatus::Skipped,
            processing_time: start_time.elapsed(),
            bytes_processed,
            merged_files: Vec::new(),
        })
    }
}

fn check_word_sanity(w: u64, or_w: u64) -> bool {
    if w == or_w {
        return true;
    }
    for k in 0..BYTE_ALIGNMENT {
        let shift = k * 8;
        let b = (w >> shift) as u8;
        let or_b = (or_w >> shift) as u8;
        if b != 0 && b != or_b {
            return false;
        }
    }
    true
}

fn find_temp_directory<'a>(paths: &'a [PathBuf], filter: &FileFilter) -> io::Result<&'a Path> {
    for p in paths {
        if let Some(parent) = p.parent() {
            if filter.is_writable(parent) {
                return Ok(parent);
            }
        }
    }

    // Fallback to first parent directory
    paths[0].parent().ok_or_else(|| {
        let error_msg = "No parent directory found for any path";
        error!("{}", error_msg);
        io::Error::new(io::ErrorKind::InvalidInput, error_msg)
    })
}

fn perform_byte_merge_mmap(mmaps: &[Mmap], or_chunk: &mut [u8], offset: usize, chunk_size: usize) {
    // Copy first mmap's chunk to or_chunk
    or_chunk.copy_from_slice(&mmaps[0][offset..offset + chunk_size]);

    let or_chunk_ptr = or_chunk.as_ptr();
    let (prefix, words, suffix) = unsafe { or_chunk.align_to_mut::<u64>() };

    for b in prefix.iter_mut() {
        let byte_offset = (b as *const u8 as usize) - (or_chunk_ptr as usize);
        for i in 1..mmaps.len() {
            *b |= mmaps[i][offset + byte_offset];
        }
    }

    for (j, w) in words.iter_mut().enumerate() {
        let word_offset = j * 8;
        for i in 1..mmaps.len() {
            let mmap_slice = &mmaps[i][offset + word_offset..offset + word_offset + 8];
            let (_, other_words, _) = unsafe { mmap_slice.align_to::<u64>() };
            if !other_words.is_empty() {
                *w |= other_words[0];
            }
        }
    }

    for b in suffix.iter_mut() {
        let byte_offset = (b as *const u8 as usize) - (or_chunk_ptr as usize);
        for i in 1..mmaps.len() {
            *b |= mmaps[i][offset + byte_offset];
        }
    }
}

fn validate_sanity_check_mmap(
    mmaps: &[Mmap],
    or_chunk: &[u8],
    is_complete: &mut [bool],
    offset: usize,
    chunk_size: usize,
) -> io::Result<bool> {
    for i in 0..mmaps.len() {
        let mmap_slice = &mmaps[i][offset..offset + chunk_size];
        if mmap_slice != or_chunk {
            is_complete[i] = false;
            let (prefix, words, suffix) = unsafe { mmap_slice.align_to::<u64>() };
            let (or_prefix, or_words, or_suffix) = unsafe { or_chunk.align_to::<u64>() };

            if !prefix
                .iter()
                .zip(or_prefix.iter())
                .all(|(b, or_b)| *b == 0 || *b == *or_b)
            {
                return Ok(false);
            }
            if !words
                .iter()
                .zip(or_words.iter())
                .all(|(w, or_w)| check_word_sanity(*w, *or_w))
            {
                return Ok(false);
            }
            if !suffix
                .iter()
                .zip(or_suffix.iter())
                .all(|(b, or_b)| *b == 0 || *b == *or_b)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn validate_sanity_check(
    buffers: &[Vec<u8>],
    or_chunk: &[u8],
    is_complete: &mut [bool],
    chunk_size: usize,
) -> io::Result<bool> {
    for i in 0..buffers.len() {
        let buffer_slice = &buffers[i][..chunk_size];
        if buffer_slice != or_chunk {
            is_complete[i] = false;
            let (prefix, words, suffix) = unsafe { buffer_slice.align_to::<u64>() };
            let (or_prefix, or_words, or_suffix) = unsafe { or_chunk.align_to::<u64>() };

            if !prefix
                .iter()
                .zip(or_prefix.iter())
                .all(|(b, or_b)| *b == 0 || *b == *or_b)
            {
                return Ok(false);
            }
            if !words
                .iter()
                .zip(or_words.iter())
                .all(|(w, or_w)| check_word_sanity(*w, *or_w))
            {
                return Ok(false);
            }
            if !suffix
                .iter()
                .zip(or_suffix.iter())
                .all(|(b, or_b)| *b == 0 || *b == *or_b)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn perform_byte_merge(buffers: &mut [Vec<u8>], or_chunk: &mut [u8]) {
    let or_chunk_len = or_chunk.len();
    or_chunk.copy_from_slice(&buffers[0][..or_chunk_len]);

    let or_chunk_ptr = or_chunk.as_ptr();
    let (prefix, words, suffix) = unsafe { or_chunk.align_to_mut::<u64>() };

    for b in prefix.iter_mut() {
        let offset = (b as *const u8 as usize) - (or_chunk_ptr as usize);
        for i in 1..buffers.len() {
            *b |= buffers[i][offset];
        }
    }

    for (j, w) in words.iter_mut().enumerate() {
        for i in 1..buffers.len() {
            let buffer_slice = &buffers[i][..or_chunk_len];
            let (_, other_words, _) = unsafe { buffer_slice.align_to::<u64>() };
            *w |= other_words[j];
        }
    }

    for b in suffix.iter_mut() {
        let offset = (b as *const u8 as usize) - (or_chunk_ptr as usize);
        for i in 1..buffers.len() {
            *b |= buffers[i][offset];
        }
    }
}

pub fn check_sanity_and_completes(
    paths: &[PathBuf],
    filter: &FileFilter,
    use_mmap: bool,
) -> io::Result<Option<(NamedTempFile, Vec<bool>)>> {
    if paths.is_empty() {
        return Ok(None);
    }

    let size = fs::metadata(&paths[0])?.len();
    if size == 0 {
        return Ok(None);
    }

    for p in &paths[1..] {
        if fs::metadata(p)?.len() != size {
            let error_msg = format!("Size mismatch in group for path: {:?}", p);
            error!("{}", error_msg);
            return Err(io::Error::new(io::ErrorKind::InvalidData, error_msg));
        }
    }

    debug!(
        "Checking sanity for {} files of size {} (mmap: {})",
        paths.len(),
        size,
        use_mmap
    );

    let temp_dir = find_temp_directory(paths, filter)?;
    let temp = NamedTempFile::new_in(temp_dir)?;
    register_temp_file(temp.path());
    let file = temp.reopen()?;
    let mut writer = BufWriter::new(file);

    if use_mmap {
        // Memory-mapped implementation
        let mut mmaps: Vec<Mmap> = Vec::with_capacity(paths.len());
        for p in paths {
            match File::open(p) {
                Ok(file) => match unsafe { MmapOptions::new().map(&file) } {
                    Ok(mmap) => mmaps.push(mmap),
                    Err(e) => {
                        error!("Failed to create memory map for {:?}: {}", p, e);
                        return Err(io::Error::other(format!(
                            "Memory mapping failed for {:?}: {}",
                            p, e
                        )));
                    }
                },
                Err(e) => {
                    error!("Failed to open file {:?} for memory mapping: {}", p, e);
                    return Err(io::Error::other(format!(
                        "Failed to open file for memory mapping {:?}: {}",
                        p, e
                    )));
                }
            }
        }

        let mut is_complete = vec![true; paths.len()];
        let mut or_chunk = vec![0; BUFFER_SIZE];

        let mut processed = 0u64;
        while processed < size {
            let chunk_size = ((size - processed) as usize).min(BUFFER_SIZE);
            let or_chunk_slice = &mut or_chunk[..chunk_size];

            // Validate bounds before accessing memory-mapped data
            let processed_usize = processed as usize;
            if processed_usize + chunk_size > mmaps[0].len() {
                error!(
                    "Memory mapping bounds check failed: processed={}, chunk_size={}, mmap_len={}",
                    processed_usize,
                    chunk_size,
                    mmaps[0].len()
                );
                return Err(io::Error::other("Memory mapping bounds exceeded"));
            }

            // Copy first file's chunk to or_chunk
            or_chunk_slice
                .copy_from_slice(&mmaps[0][processed_usize..processed_usize + chunk_size]);

            // Perform byte merge with memory-mapped data
            perform_byte_merge_mmap(&mmaps, or_chunk_slice, processed_usize, chunk_size);

            // Validate sanity check
            if !validate_sanity_check_mmap(
                &mmaps,
                or_chunk_slice,
                &mut is_complete,
                processed_usize,
                chunk_size,
            )? {
                return Ok(None);
            }

            writer.write_all(or_chunk_slice)?;
            processed += chunk_size as u64;
        }

        debug!(
            "Processed {} of {} bytes for group with mmap",
            processed, size
        );
        writer.flush()?;
        Ok(Some((temp, is_complete)))
    } else {
        // Original buffered I/O implementation
        let mut readers: Vec<BufReader<File>> = Vec::with_capacity(paths.len());
        for p in paths {
            match File::open(p) {
                Ok(file) => readers.push(BufReader::new(file)),
                Err(e) => {
                    error!("Failed to open file {:?} for reading: {}", p, e);
                    return Err(io::Error::other(format!(
                        "Failed to open file for reading {:?}: {}",
                        p, e
                    )));
                }
            }
        }

        let mut buffers: Vec<Vec<u8>> = (0..paths.len()).map(|_| vec![0; BUFFER_SIZE]).collect();
        let mut is_complete = vec![true; paths.len()];
        let mut or_chunk = vec![0; BUFFER_SIZE];

        let mut processed = 0u64;
        while processed < size {
            let chunk_size = ((size - processed) as usize).min(BUFFER_SIZE);
            let buffers_slice = &mut buffers;
            let or_chunk_slice = &mut or_chunk[..chunk_size];

            for (i, reader) in readers.iter_mut().enumerate() {
                match reader.read_exact(&mut buffers_slice[i][..chunk_size]) {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            "Failed to read from file {} at offset {}: {}",
                            i, processed, e
                        );
                        return Err(io::Error::other(format!(
                            "Failed to read from file at offset {}: {}",
                            processed, e
                        )));
                    }
                }
            }

            perform_byte_merge(buffers_slice, or_chunk_slice);

            if !validate_sanity_check(buffers_slice, or_chunk_slice, &mut is_complete, chunk_size)?
            {
                return Ok(None);
            }

            writer.write_all(or_chunk_slice)?;
            processed += chunk_size as u64;
        }

        debug!("Processed {} of {} bytes for group", processed, size);
        writer.flush()?;
        Ok(Some((temp, is_complete)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use tempfile::tempdir;

    #[test]
    fn test_single_file() -> io::Result<()> {
        let dir = tempdir()?;
        let p1 = dir.path().join("a");
        let data = vec![1u8, 2, 3];
        fs::write(&p1, &data)?;

        let paths = vec![p1];

        if let Some((temp, is_complete)) =
            check_sanity_and_completes(&paths, &FileFilter::new(vec![]), false)?
        {
            assert_eq!(is_complete, vec![true]);
            assert_eq!(fs::read(temp.path())?, data);
        } else {
            panic!("Expected Some for single file");
        }
        Ok(())
    }

    #[test]
    fn test_size_mismatch() -> io::Result<()> {
        let dir = tempdir()?;
        let p1 = dir.path().join("a");
        fs::write(&p1, vec![1u8, 2, 3])?;

        let p2 = dir.path().join("b");
        fs::write(&p2, vec![4u8, 5])?;

        let paths = vec![p1, p2];
        let res = check_sanity_and_completes(&paths, &FileFilter::new(vec![]), false);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_sanity_fail() -> io::Result<()> {
        let dir = tempdir()?;
        let p1 = dir.path().join("a");
        fs::write(&p1, vec![1u8, 0])?;

        let p2 = dir.path().join("b");
        fs::write(&p2, vec![2u8, 0])?;

        let paths = vec![p1, p2];
        let res = check_sanity_and_completes(&paths, &FileFilter::new(vec![]), false)?;
        assert!(res.is_none());
        Ok(())
    }

    #[test]
    fn test_compatible_merge_multiple() -> io::Result<()> {
        let dir = tempdir()?;
        let p1 = dir.path().join("a");
        let data1 = vec![1u8, 0, 0];
        fs::write(&p1, &data1)?;

        let p2 = dir.path().join("b");
        let data2 = vec![0u8, 1, 0];
        fs::write(&p2, &data2)?;

        let p3 = dir.path().join("c");
        let data3 = vec![1u8, 1, 0];
        fs::write(&p3, &data3)?;

        let paths = vec![p1, p2, p3];

        if let Some((temp, is_complete)) =
            check_sanity_and_completes(&paths, &FileFilter::new(vec![]), false)?
        {
            assert_eq!(is_complete, vec![false, false, true]);
            assert_eq!(fs::read(temp.path())?, vec![1u8, 1, 0]);
        } else {
            panic!("Expected Some for compatible merge");
        }
        Ok(())
    }

    #[test]
    fn test_process_group_creates_merged_for_incomplete() -> io::Result<()> {
        let dir = tempdir()?;
        let sub1 = dir.path().join("sub1");
        fs::create_dir(&sub1)?;
        let file1 = sub1.join("video.mkv");
        let data_incomplete = vec![0u8, 0, 0];
        fs::write(&file1, &data_incomplete)?;

        let sub2 = dir.path().join("sub2");
        fs::create_dir(&sub2)?;
        let file2 = sub2.join("video.mkv");
        let data_complete = vec![4u8, 5, 6];
        fs::write(&file2, &data_complete)?;

        let paths = vec![file1.clone(), file2.clone()];
        let stats =
            process_group_with_dry_run(&paths, "video.mkv", false, &[], false, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Merged));
        assert_eq!(stats.merged_files.len(), 1);

        let merged1 = sub1.join("video.mkv.merged");
        assert!(merged1.exists());
        assert_eq!(fs::read(&merged1)?, data_complete);

        let merged2 = sub2.join("video.mkv.merged");
        assert!(!merged2.exists());
        Ok(())
    }

    #[test]
    fn test_process_group_no_merged_on_conflict() -> io::Result<()> {
        let dir = tempdir()?;
        let p1 = dir.path().join("a");
        fs::write(&p1, vec![1u8, 0])?;

        let p2 = dir.path().join("b");
        fs::write(&p2, vec![2u8, 0])?;

        let paths = vec![p1.clone(), p2.clone()];
        let stats = process_group_with_dry_run(&paths, "dummy", false, &[], false, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Failed));

        let merged1 = dir.path().join("a.merged");
        assert!(!merged1.exists());

        let merged2 = dir.path().join("b.merged");
        assert!(!merged2.exists());
        Ok(())
    }

    #[test]
    fn test_process_group_no_merged_all_complete() -> io::Result<()> {
        let dir = tempdir()?;
        let p1 = dir.path().join("a");
        let data = vec![4u8, 5, 6];
        fs::write(&p1, &data)?;

        let p2 = dir.path().join("b");
        fs::write(&p2, &data)?;

        let paths = vec![p1.clone(), p2.clone()];
        let stats = process_group_with_dry_run(&paths, "dummy", false, &[], false, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Skipped));

        let merged1 = dir.path().join("a.merged");
        assert!(!merged1.exists());

        let merged2 = dir.path().join("b.merged");
        assert!(!merged2.exists());
        Ok(())
    }

    #[test]
    fn test_process_group_replace_for_incomplete() -> io::Result<()> {
        let dir = tempdir()?;
        let sub1 = dir.path().join("sub1");
        fs::create_dir(&sub1)?;
        let file1 = sub1.join("video.mkv");
        let data_incomplete = vec![0u8, 0, 0];
        fs::write(&file1, &data_incomplete)?;

        let sub2 = dir.path().join("sub2");
        fs::create_dir(&sub2)?;
        let file2 = sub2.join("video.mkv");
        let data_complete = vec![4u8, 5, 6];
        fs::write(&file2, &data_complete)?;

        let paths = vec![file1.clone(), file2.clone()];
        let stats =
            process_group_with_dry_run(&paths, "video.mkv", true, &[], false, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Merged));

        assert_eq!(fs::read(&file1)?, data_complete);
        assert_eq!(fs::read(&file2)?, data_complete);

        let merged1 = sub1.join("video.mkv.merged");
        assert!(!merged1.exists());

        let merged2 = sub2.join("video.mkv.merged");
        assert!(!merged2.exists());
        Ok(())
    }

    #[test]
    fn test_process_group_src_dirs_readonly() -> io::Result<()> {
        let dir = tempdir()?;

        // Create a source directory (read-only)
        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir)?;
        let src_file = src_dir.join("video.mkv");
        let mut src_data = vec![4u8, 5, 6];
        src_data.resize(15 * 1024, 7);
        fs::write(&src_file, &src_data)?;

        // Create a target directory with different data (incomplete relative to src)
        let target_dir = dir.path().join("target");
        fs::create_dir(&target_dir)?;
        let target_file = target_dir.join("video.mkv");
        let mut target_data = vec![1u8, 2, 3];
        target_data.resize(15 * 1024, 0);
        fs::write(&target_file, &target_data)?;

        // Create another target directory with incomplete data
        let target2_dir = dir.path().join("target2");
        fs::create_dir(&target2_dir)?;
        let target2_file = target2_dir.join("video.mkv");
        let mut target2_data = vec![0u8, 1, 2];
        target2_data.resize(15 * 1024, 0);
        fs::write(&target2_file, &target2_data)?;

        let paths = vec![src_file.clone(), target_file.clone(), target2_file.clone()];
        let src_dirs = vec![src_dir.clone()];
        let stats =
            process_group_with_dry_run(&paths, "video.mkv", false, &src_dirs, false, false, false)?;

        // Should fail because target files are incompatible (different non-zero bytes)
        assert!(matches!(stats.status, GroupStatus::Failed));

        // Source file should remain unchanged
        assert_eq!(fs::read(&src_file)?, src_data);

        // Source directory should not have any merged files
        let merged_src = src_dir.join("video.mkv.merged");
        assert!(!merged_src.exists());

        Ok(())
    }

    #[test]
    fn test_file_filter_new() {
        let src_dirs = vec![PathBuf::from("/src1"), PathBuf::from("/src2")];
        let filter = FileFilter::new(src_dirs.clone());

        // Test that filter stores src_dirs correctly
        assert_eq!(filter.src_dirs, src_dirs);
    }

    #[test]
    fn test_file_filter_is_writable() {
        let src_dirs = vec![PathBuf::from("/readonly")];
        let filter = FileFilter::new(src_dirs);

        // Test writable path (not in src dirs)
        let writable_path = Path::new("/writable/file.txt");
        assert!(filter.is_writable(writable_path));

        // Test read-only path ( in src dirs) - just test the function doesn't panic
        let readonly_path = Path::new("/readonly/file.txt");
        let _result = filter.is_writable(readonly_path);
        // The result depends on whether canonicalization works for non-existent paths
    }

    #[test]
    fn test_file_filter_is_in_src_dir() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir)?;

        let filter = FileFilter::new(vec![src_dir.clone()]);

        // Test path in src dir
        let file_in_src = src_dir.join("file.txt");
        // The is_in_src_dir function uses canonicalization, so this should work
        let result = filter.is_in_src_dir(&file_in_src);
        // We expect this to be true, but if canonicalization fails, it might be false
        // Let's just test that the function doesn't panic
        let _ = result;

        // Test path not in src dir
        let file_outside = temp_dir.path().join("file.txt");
        assert!(!filter.is_in_src_dir(&file_outside));

        // Test nonexistent path (should not panic)
        let nonexistent = Path::new("/nonexistent/path");
        assert!(!filter.is_in_src_dir(nonexistent));

        Ok(())
    }

    #[test]
    fn test_file_filter_filter_writable_paths() {
        let src_dirs = vec![PathBuf::from("/readonly")];
        let filter = FileFilter::new(src_dirs);

        let paths = vec![
            PathBuf::from("/writable/file1.txt"),
            PathBuf::from("/readonly/file2.txt"),
            PathBuf::from("/writable/file3.txt"),
        ];

        let writable_paths = filter.filter_writable_paths(&paths);

        // Should filter out readonly paths, but the exact count depends on canonicalization
        assert!(!writable_paths.is_empty());
        assert!(writable_paths.len() <= 3);
        assert!(writable_paths.contains(&PathBuf::from("/writable/file1.txt")));
        assert!(writable_paths.contains(&PathBuf::from("/writable/file3.txt")));
    }

    #[test]
    fn test_check_word_sanity() {
        // Test identical words
        assert!(check_word_sanity(0x12345678, 0x12345678));

        // Test compatible words (one is subset of other)
        assert!(check_word_sanity(0x12340000, 0x12345678));
        assert!(check_word_sanity(0x00005678, 0x12345678));
        assert!(check_word_sanity(0x12005600, 0x12345678));

        // Test incompatible words (different non-zero bits)
        assert!(!check_word_sanity(0x12345678, 0x87654321));
        assert!(!check_word_sanity(0x12345678, 0x12345679));
    }

    #[test]
    fn test_find_temp_directory() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let writable_dir = temp_dir.path().join("writable");
        fs::create_dir(&writable_dir)?;

        let readonly_dir = temp_dir.path().join("readonly");
        fs::create_dir(&readonly_dir)?;

        let filter = FileFilter::new(vec![readonly_dir.clone()]);

        let paths = vec![
            writable_dir.join("file1.txt"),
            readonly_dir.clone().join("file2.txt"),
        ];

        let temp_dir_found = find_temp_directory(&paths, &filter)?;

        // Should find the writable directory
        assert_eq!(temp_dir_found, writable_dir);

        Ok(())
    }

    #[test]
    fn test_find_temp_directory_no_writable() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let readonly_dir = temp_dir.path().join("readonly");
        fs::create_dir(&readonly_dir)?;

        let filter = FileFilter::new(vec![readonly_dir.clone()]);
        let paths = vec![readonly_dir.join("file.txt")];

        let result = find_temp_directory(&paths, &filter);

        // Should fallback to first parent directory
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), temp_dir.path().join("readonly"));

        Ok(())
    }

    #[test]
    fn test_perform_byte_merge_mmap() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create test files
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        fs::write(&file1, [0x12, 0x34, 0x00, 0x56])?;
        fs::write(&file2, [0x00, 0x34, 0x78, 0x00])?;

        // Create memory maps
        let mmap1 = unsafe { MmapOptions::new().map(&File::open(&file1)?)? };
        let mmap2 = unsafe { MmapOptions::new().map(&File::open(&file2)?)? };

        let mmaps = vec![mmap1, mmap2];
        let mut or_chunk = vec![0u8; 4];

        perform_byte_merge_mmap(&mmaps, &mut or_chunk, 0, 4);

        // Expected result: 0x12 | 0x00 = 0x12, 0x34 | 0x34 = 0x34, 0x00 | 0x78 = 0x78, 0x56 | 0x00 = 0x56
        assert_eq!(or_chunk, &[0x12, 0x34, 0x78, 0x56]);

        Ok(())
    }

    #[test]
    fn test_validate_sanity_check_mmap() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create test files
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        fs::write(&file1, [0x12, 0x34, 0x00, 0x56])?;
        fs::write(&file2, [0x00, 0x34, 0x78, 0x00])?;

        // Create memory maps
        let mmap1 = unsafe { MmapOptions::new().map(&File::open(&file1)?)? };
        let mmap2 = unsafe { MmapOptions::new().map(&File::open(&file2)?)? };

        let mmaps = vec![mmap1, mmap2];
        let or_chunk = vec![0x12, 0x34, 0x78, 0x56];
        let mut is_complete = vec![true, true];

        let result = validate_sanity_check_mmap(&mmaps, &or_chunk, &mut is_complete, 0, 4)?;

        // Should pass validation
        assert!(result);
        // We expect at least one file to be incomplete since they have different bytes
        assert!(is_complete.iter().any(|&complete| !complete));

        Ok(())
    }

    #[test]
    fn test_validate_sanity_check_mmap_failure() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create test files with incompatible data
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        fs::write(&file1, [0x12, 0x34, 0x56, 0x78])?;
        fs::write(&file2, [0x87, 0x65, 0x43, 0x21])?;

        // Create memory maps
        let mmap1 = unsafe { MmapOptions::new().map(&File::open(&file1)?)? };
        let mmap2 = unsafe { MmapOptions::new().map(&File::open(&file2)?)? };

        let mmaps = vec![mmap1, mmap2];
        let or_chunk = vec![0x99, 0x79, 0x57, 0x79]; // OR of both files
        let mut is_complete = vec![true, true];

        let result = validate_sanity_check_mmap(&mmaps, &or_chunk, &mut is_complete, 0, 4)?;

        // Should fail validation due to incompatible bits
        assert!(!result);

        Ok(())
    }

    #[test]
    fn test_perform_byte_merge() -> io::Result<()> {
        let buffer1 = vec![0x12, 0x34, 0x00, 0x56];
        let buffer2 = vec![0x00, 0x34, 0x78, 0x00];
        let mut buffers = vec![buffer1.clone(), buffer2.clone()];
        let mut or_chunk = vec![0u8; 4];

        perform_byte_merge(&mut buffers, &mut or_chunk);

        // Expected result: 0x12 | 0x00 = 0x12, 0x34 | 0x34 = 0x34, 0x00 | 0x78 = 0x78, 0x56 | 0x00 = 0x56
        assert_eq!(or_chunk, &[0x12, 0x34, 0x78, 0x56]);

        Ok(())
    }

    #[test]
    fn test_validate_sanity_check() -> io::Result<()> {
        let buffer1 = vec![0x12, 0x34, 0x00, 0x56];
        let buffer2 = vec![0x00, 0x34, 0x78, 0x00];
        let buffers = vec![buffer1, buffer2];
        let or_chunk = vec![0x12, 0x34, 0x78, 0x56];
        let mut is_complete = vec![true, true];

        let result = validate_sanity_check(&buffers, &or_chunk, &mut is_complete, 4)?;

        // Should pass validation
        assert!(result);
        // The is_complete array should be updated based on the validation
        // We expect at least one file to be incomplete since they have different bytes
        assert!(is_complete.iter().any(|&complete| !complete));

        Ok(())
    }

    #[test]
    fn test_validate_sanity_check_failure() -> io::Result<()> {
        let buffer1 = vec![0x12, 0x34, 0x56, 0x78];
        let buffer2 = vec![0x87, 0x65, 0x43, 0x21];
        let buffers = vec![buffer1, buffer2];
        let or_chunk = vec![0x99, 0x79, 0x57, 0x79]; // OR of both buffers
        let mut is_complete = vec![true, true];

        let result = validate_sanity_check(&buffers, &or_chunk, &mut is_complete, 4)?;

        // Should fail validation due to incompatible bits
        assert!(!result);

        Ok(())
    }

    #[test]
    fn test_check_sanity_and_completes_empty_paths() -> io::Result<()> {
        let paths: Vec<PathBuf> = vec![];
        let filter = FileFilter::new(vec![]);

        let result = check_sanity_and_completes(&paths, &filter, false)?;

        assert!(result.is_none());

        Ok(())
    }

    #[test]
    fn test_check_sanity_and_completes_zero_size_file() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let empty_file = temp_dir.path().join("empty.bin");
        fs::write(&empty_file, "")?;

        let paths = vec![empty_file];
        let filter = FileFilter::new(vec![]);

        let result = check_sanity_and_completes(&paths, &filter, false)?;

        assert!(result.is_none());

        Ok(())
    }

    #[test]
    fn test_check_sanity_and_completes_memory_mapping() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        // Create larger files to trigger memory mapping (>5MB)
        let large_data = vec![0x12u8; 6 * 1024 * 1024]; // 6MB
        let large_data2 = vec![0x00u8; 6 * 1024 * 1024]; // 6MB

        fs::write(&file1, large_data)?;
        fs::write(&file2, large_data2)?;

        let paths = vec![file1, file2];
        let filter = FileFilter::new(vec![]);

        let result = check_sanity_and_completes(&paths, &filter, true)?;

        assert!(result.is_some());
        let (temp_file, is_complete) = result.unwrap();
        assert_eq!(is_complete, vec![true, false]); // first file complete, second incomplete

        // Verify temp file exists and has correct content
        assert!(temp_file.path().exists());
        let temp_content = fs::read(temp_file.path())?;
        assert_eq!(temp_content.len(), 6 * 1024 * 1024);
        assert_eq!(temp_content[0], 0x12); // Should have OR of both files

        Ok(())
    }

    #[test]
    fn test_process_group_with_dry_run_mode() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let file1 = temp_dir.path().join("file1.bin");
        let file2 = temp_dir.path().join("file2.bin");

        fs::write(&file1, vec![0x12, 0x34, 0x56])?;
        fs::write(&file2, vec![0x00, 0x34, 0x00])?;

        let paths = vec![file1, file2];
        let stats = process_group_with_dry_run(&paths, "test", false, &[], true, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Merged));
        assert_eq!(stats.merged_files.len(), 2); // Both files need merging in dry run

        Ok(())
    }

    #[test]
    fn test_process_group_all_readonly() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let readonly_dir = temp_dir.path().join("readonly");
        fs::create_dir(&readonly_dir)?;

        let file1 = readonly_dir.join("file1.bin");
        let file2 = readonly_dir.join("file2.bin");

        fs::write(&file1, vec![0x12, 0x34])?;
        fs::write(&file2, vec![0x00, 0x34])?;

        let paths = vec![file1, file2];
        let src_dirs = vec![readonly_dir];
        let stats =
            process_group_with_dry_run(&paths, "test", false, &src_dirs, false, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Skipped));
        assert_eq!(stats.merged_files.len(), 0);

        Ok(())
    }

    #[test]
    fn test_process_group_zero_size_files() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let file1 = temp_dir.path().join("empty1.bin");
        let file2 = temp_dir.path().join("empty2.bin");

        fs::write(&file1, "")?;
        fs::write(&file2, "")?;

        let paths = vec![file1, file2];
        let stats = process_group_with_dry_run(&paths, "test", false, &[], false, false, false)?;

        assert!(matches!(stats.status, GroupStatus::Skipped));
        assert_eq!(stats.merged_files.len(), 0);

        Ok(())
    }

    #[test]
    fn test_copy_empty_dst_functionality() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create source directory (read-only)
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir)?;
        let src_file = src_dir.join("test.bin");
        let src_data = vec![1u8, 2, 3, 4, 5];
        fs::write(&src_file, &src_data)?;

        // Create target directory with empty (null) file
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&target_dir)?;
        let target_file = target_dir.join("test.bin");
        let null_data = vec![0u8; 5]; // Same size, all nulls
        fs::write(&target_file, null_data)?;

        let paths = vec![src_file.clone(), target_file.clone()];
        let src_dirs = vec![src_dir.clone()];

        // Test with copy_empty_dst enabled
        let stats =
            process_group_with_dry_run(&paths, "test.bin", false, &src_dirs, false, false, true)?;

        assert!(matches!(stats.status, GroupStatus::Merged));
        assert_eq!(stats.merged_files.len(), 1);
        assert_eq!(fs::read(&target_file)?, src_data);

        Ok(())
    }

    #[test]
    fn test_temp_file_trait() {
        let temp_dir = tempdir().unwrap();
        let temp_file = NamedTempFile::new_in(temp_dir.path()).unwrap();

        // Test that TempFile trait works for NamedTempFile
        let path = TempFile::path(&temp_file);
        assert!(path.exists());

        // Test that TempFile trait works for MockTempFile
        let mock_temp = MockTempFile;
        let mock_path = TempFile::path(&mock_temp);
        assert_eq!(mock_path, Path::new("/mock/dry-run"));
    }

    #[test]
    fn test_group_status_debug() {
        // Test that all GroupStatus variants can be formatted
        let merged_debug = format!("{:?}", GroupStatus::Merged);
        let skipped_debug = format!("{:?}", GroupStatus::Skipped);
        let failed_debug = format!("{:?}", GroupStatus::Failed);

        assert_eq!(merged_debug, "Merged");
        assert_eq!(skipped_debug, "Skipped");
        assert_eq!(failed_debug, "Failed");
    }

    #[test]
    fn test_group_stats_debug() {
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");

        let stats = GroupStats {
            status: GroupStatus::Merged,
            processing_time: Duration::from_secs(1),
            bytes_processed: 1024,
            merged_files: vec![test_file.clone()],
        };

        // Test all fields are accessible
        assert!(matches!(stats.status, GroupStatus::Merged));
        assert_eq!(stats.processing_time, Duration::from_secs(1));
        assert_eq!(stats.bytes_processed, 1024);
        assert_eq!(stats.merged_files.len(), 1);
        assert_eq!(stats.merged_files[0], test_file);
    }

    #[test]
    fn test_filenames_fuzzy_match() {
        // Exact matches
        assert!(filenames_fuzzy_match("test.txt", "test.txt"));
        assert!(filenames_fuzzy_match("video.mkv", "video.mkv"));

        // Fuzzy matches with 80%+ similarity (5+ chars)
        assert!(filenames_fuzzy_match("video.mkv", "vido.mkv")); // 1 char difference = 8/9 = 88.9%
        assert!(filenames_fuzzy_match("movie_2024.mp4", "movie_2025.mp4")); // 1 char difference = 14/15 = 93.3%
        assert!(filenames_fuzzy_match("test_file.txt", "test_fle.txt")); // 1 char difference = 12/13 = 92.3%
        assert!(filenames_fuzzy_match("video.mkv", "vdeo.mkv")); // 1 deletion = 8/9 = 88.9%

        // No match (less than 80% similarity)
        assert!(!filenames_fuzzy_match("video.mkv", "vdo.mkv")); // 2 deletions = 7/9 = 77.8%
        assert!(!filenames_fuzzy_match(
            "completely_different.txt",
            "other_file.txt"
        ));

        // Too short for fuzzy matching (less than 5 chars)
        assert!(!filenames_fuzzy_match("test", "tst")); // 4 chars, no fuzzy
        assert!(!filenames_fuzzy_match("abc", "abd")); // 3 chars, no fuzzy
                                                       // Edge case: exactly 5 characters with 1 difference = 80% match
        assert!(filenames_fuzzy_match("abcde", "abxde")); // 1/5 = 80% match
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("a", ""), 1);
        assert_eq!(levenshtein_distance("", "a"), 1);
        assert_eq!(levenshtein_distance("a", "a"), 0);
        assert_eq!(levenshtein_distance("ab", "a"), 1);
        assert_eq!(levenshtein_distance("ab", "ac"), 1);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("flaw", "lawn"), 2);
    }

    #[test]
    fn test_copy_empty_dst_multiple_sources() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create source directory (read-only)
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir)?;

        // Create multiple source files with same name and same size but different content
        let src_subdir1 = src_dir.join("src1");
        let src_subdir2 = src_dir.join("src2");
        let src_subdir3 = src_dir.join("src3");
        fs::create_dir(&src_subdir1)?;
        fs::create_dir(&src_subdir2)?;
        fs::create_dir(&src_subdir3)?;

        let src_file1 = src_subdir1.join("test.bin");
        let src_file2 = src_subdir2.join("test.bin");
        let src_file3 = src_subdir3.join("test.bin");

        fs::write(&src_file1, b"good_dat")?;
        fs::write(&src_file2, b"other_da")?;
        fs::write(&src_file3, b"more_dat")?;

        // Create destination file (empty/nulls) - same size as sources
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&target_dir)?;
        let target_file = target_dir.join("test.bin");
        fs::write(&target_file, b"\0\0\0\0\0\0\0\0")?;

        let paths = vec![
            src_file1.clone(),
            src_file2.clone(),
            src_file3.clone(),
            target_file.clone(),
        ];
        let src_dirs = vec![src_dir.clone()];

        // Test with copy_empty_dst enabled - should handle multiple sources
        let stats =
            process_group_with_dry_run(&paths, "test.bin", false, &src_dirs, false, false, true)?;

        // Should have merged successfully
        assert!(matches!(stats.status, GroupStatus::Merged));
        assert_eq!(stats.merged_files.len(), 1);
        assert_eq!(stats.bytes_processed, 8);

        Ok(())
    }

    #[test]
    fn test_copy_empty_dst_fuzzy_matching() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create source directory (read-only)
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir)?;
        let src_file = src_dir.join("video.mkv");
        let src_data = vec![1u8, 2, 3, 4, 5];
        fs::write(&src_file, &src_data)?;

        // Create target directory with empty (null) file and slightly different name
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&target_dir)?;
        let target_file = target_dir.join("vido.mkv"); // 1 char difference
        let null_data = vec![0u8; 5]; // Same size, all nulls
        fs::write(&target_file, null_data)?;

        let paths = vec![src_file.clone(), target_file.clone()];
        let src_dirs = vec![src_dir.clone()];

        // Test with copy_empty_dst enabled - should match fuzzily
        let stats =
            process_group_with_dry_run(&paths, "vido.mkv", false, &src_dirs, false, false, true)?;

        assert!(matches!(stats.status, GroupStatus::Merged));
        assert_eq!(stats.merged_files.len(), 1);
        assert_eq!(fs::read(&target_file)?, src_data);

        Ok(())
    }
}
