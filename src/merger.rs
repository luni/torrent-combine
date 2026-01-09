use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::error;
use tempfile::NamedTempFile;

fn is_in_src_dir(path: &Path, src_dirs: &[PathBuf]) -> bool {
    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    src_dirs.iter().any(|src_dir| {
        if let Ok(canonical_src) = src_dir.canonicalize() {
            canonical_path.starts_with(canonical_src)
        } else {
            false
        }
    })
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

pub fn process_group(paths: &[PathBuf], basename: &str, replace: bool, src_dirs: &[PathBuf]) -> io::Result<GroupStats> {
    let start_time = Instant::now();
    log::debug!("Processing paths for group {}: {:?}", basename, paths);

    // Filter out files that are in src directories (read-only)
    let writable_paths: Vec<PathBuf> = paths.iter()
        .filter(|path| !is_in_src_dir(path, src_dirs))
        .cloned()
        .collect();

    if writable_paths.is_empty() {
        log::info!("All files in group '{}' are in read-only src directories, skipping", basename);
        return Ok(GroupStats {
            status: GroupStatus::Skipped,
            processing_time: start_time.elapsed(),
            bytes_processed: 0,
            merged_files: Vec::new(),
        });
    }

    log::info!("Processing {} writable files out of {} total for group '{}'", writable_paths.len(), paths.len(), basename);

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

    let res = check_sanity_and_completes(&writable_paths, src_dirs)?;

    if let Some((temp, is_complete)) = res {
        log::info!("Sanity check passed for group {}", basename);

        let any_incomplete = is_complete.iter().any(|&c| !c);
        if any_incomplete {
            let mut merged_files = Vec::new();
            for (j, &complete) in is_complete.iter().enumerate() {
                if !complete {
                    let path = &writable_paths[j];

                    // Skip files that are in src directories (read-only)
                    if is_in_src_dir(path, src_dirs) {
                        log::info!("Skipping read-only file in src directory: {:?}", path);
                        continue;
                    }

                    let parent = path.parent().ok_or(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "No parent directory",
                    ))?;

                    // Additional safety check: ensure parent is not in src dirs
                    if is_in_src_dir(parent, src_dirs) {
                        log::info!("Skipping file because parent directory is in src directories: {:?}", parent);
                        continue;
                    }

                    let local_temp = NamedTempFile::new_in(parent)?;
                    fs::copy(temp.path(), local_temp.path())?;
                    if replace {
                        fs::rename(local_temp.path(), path)?;
                        log::debug!("Replaced original {:?} with merged content", path);
                    } else {
                        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
                        let merged_path = parent.join(format!("{}.merged", file_name));
                        local_temp.persist(&merged_path)?;
                        log::debug!(
                            "Created merged file {:?} for incomplete original {:?}",
                            merged_path,
                            path
                        );
                        merged_files.push(merged_path);
                    }
                }
            }
            log::info!(
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
            log::info!(
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
    } else {
        error!("Failed sanity check for group: {}", basename);
        Ok(GroupStats {
            status: GroupStatus::Failed,
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
    for k in 0..8 {
        let shift = k * 8;
        let b = (w >> shift) as u8;
        let or_b = (or_w >> shift) as u8;
        if b != 0 && b != or_b {
            return false;
        }
    }
    true
}

fn check_sanity_and_completes(paths: &[PathBuf], src_dirs: &[PathBuf]) -> io::Result<Option<(NamedTempFile, Vec<bool>)>> {
    if paths.is_empty() {
        return Ok(None);
    }

    let size = fs::metadata(&paths[0])?.len();
    if size == 0 {
        return Ok(None);
    }

    for p in &paths[1..] {
        if fs::metadata(p)?.len() != size {
            log::error!("Size mismatch in group for path {:?}", p);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Size mismatch in group",
            ));
        }
    }

    log::debug!("Checking sanity for {} files of size {}", paths.len(), size);

    // Find a suitable parent directory for temp file (prefer non-src directory)
    let mut temp_dir = None;
    for p in paths {
        if let Some(parent) = p.parent() {
            if !is_in_src_dir(parent, src_dirs) {
                temp_dir = Some(parent);
                break;
            } else if temp_dir.is_none() {
                temp_dir = Some(parent);
            }
        }
    }

    let temp_dir = temp_dir.ok_or(io::Error::new(
        io::ErrorKind::InvalidInput,
        "No parent directory found for any path",
    ))?;
    let temp = NamedTempFile::new_in(temp_dir)?;
    let file = temp.reopen()?;
    let mut writer = BufWriter::new(file);

    let mut readers: Vec<BufReader<File>> = Vec::with_capacity(paths.len());
    for p in paths {
        readers.push(BufReader::new(File::open(p)?));
    }

    const BUF_SIZE: usize = 1 << 20;
    let mut buffers: Vec<Vec<u8>> = (0..paths.len()).map(|_| vec![0; BUF_SIZE]).collect();
    let mut is_complete = vec![true; paths.len()];
    let mut or_chunk = vec![0; BUF_SIZE];

    let mut processed = 0u64;
    while processed < size {
        let chunk_size = ((size - processed) as usize).min(BUF_SIZE);
        let buffers_slice = &mut buffers;
        let or_chunk_slice = &mut or_chunk[..chunk_size];

        for (i, reader) in readers.iter_mut().enumerate() {
            reader.read_exact(&mut buffers_slice[i][..chunk_size])?;
        }

        or_chunk_slice.copy_from_slice(&buffers_slice[0][..chunk_size]);

        let or_chunk_ptr = or_chunk_slice.as_ptr();
        let (prefix, words, suffix) = unsafe { or_chunk_slice.align_to_mut::<u64>() };

        for b in prefix.iter_mut() {
            let offset = (b as *const u8 as usize) - (or_chunk_ptr as usize);
            for i in 1..paths.len() {
                *b |= buffers_slice[i][offset];
            }
        }
        for (j, w) in words.iter_mut().enumerate() {
            for i in 1..paths.len() {
                let (_, other_words, _) =
                    unsafe { buffers_slice[i][..chunk_size].align_to::<u64>() };
                *w |= other_words[j];
            }
        }
        for b in suffix.iter_mut() {
            let offset = (b as *const u8 as usize) - (or_chunk_ptr as usize);
            for i in 1..paths.len() {
                *b |= buffers_slice[i][offset];
            }
        }

        for i in 0..paths.len() {
            let buffer_slice = &buffers_slice[i][..chunk_size];
            if buffer_slice != or_chunk_slice {
                is_complete[i] = false;
                let (prefix, words, suffix) = unsafe { buffer_slice.align_to::<u64>() };
                let (or_prefix, or_words, or_suffix) = unsafe { or_chunk_slice.align_to::<u64>() };

                if !prefix
                    .iter()
                    .zip(or_prefix.iter())
                    .all(|(b, or_b)| *b == 0 || *b == *or_b)
                {
                    return Ok(None);
                }
                if !words
                    .iter()
                    .zip(or_words.iter())
                    .all(|(w, or_w)| check_word_sanity(*w, *or_w))
                {
                    return Ok(None);
                }
                if !suffix
                    .iter()
                    .zip(or_suffix.iter())
                    .all(|(b, or_b)| *b == 0 || *b == *or_b)
                {
                    return Ok(None);
                }
            }
        }

        writer.write_all(or_chunk_slice)?;
        processed += chunk_size as u64;
    }

    log::debug!("Processed {} of {} bytes for group", processed, size);
    writer.flush()?;
    Ok(Some((temp, is_complete)))
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

        if let Some((temp, is_complete)) = check_sanity_and_completes(&paths, &[])? {
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
        let res = check_sanity_and_completes(&paths, &[]);
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
        let res = check_sanity_and_completes(&paths, &[])?;
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

        if let Some((temp, is_complete)) = check_sanity_and_completes(&paths, &[])? {
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
        let stats = process_group(&paths, "video.mkv", false, &[])?;

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
        let stats = process_group(&paths, "dummy", false, &[])?;

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
        let stats = process_group(&paths, "dummy", false, &[])?;

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
        let stats = process_group(&paths, "video.mkv", true, &[])?;

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
        let stats = process_group(&paths, "video.mkv", false, &src_dirs)?;

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
