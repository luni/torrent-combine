#![allow(clippy::needless_range_loop)]

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::{debug, error, info};
use memmap2::{Mmap, MmapOptions};
use tempfile::NamedTempFile;

// Register temp files for cleanup
fn register_temp_file(path: &Path) {
    crate::register_temp_file(path.to_path_buf());
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
            let error_msg = format!("Sanity check failed for group: {}", basename);
            error!("{}", error_msg);
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
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Memory mapping failed for {:?}: {}", p, e),
                        ));
                    }
                },
                Err(e) => {
                    error!("Failed to open file {:?} for memory mapping: {}", p, e);
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to open file for memory mapping {:?}: {}", p, e),
                    ));
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
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Memory mapping bounds exceeded",
                ));
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
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to open file for reading {:?}: {}", p, e),
                    ));
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
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Failed to read from file at offset {}: {}", processed, e),
                        ));
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
        let stats = process_group_with_dry_run(&paths, "video.mkv", false, &[], false, false)?;

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
        let stats = process_group_with_dry_run(&paths, "dummy", false, &[], false, false)?;

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
        let stats = process_group_with_dry_run(&paths, "dummy", false, &[], false, false)?;

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
        let stats = process_group_with_dry_run(&paths, "video.mkv", true, &[], false, false)?;

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
            process_group_with_dry_run(&paths, "video.mkv", false, &src_dirs, false, false)?;

        // Should fail because target files are incompatible (different non-zero bytes)
        assert!(matches!(stats.status, GroupStatus::Failed));

        // Source file should remain unchanged
        assert_eq!(fs::read(&src_file)?, src_data);

        // Source directory should not have any merged files
        let merged_src = src_dir.join("video.mkv.merged");
        assert!(!merged_src.exists());

        Ok(())
    }
}
