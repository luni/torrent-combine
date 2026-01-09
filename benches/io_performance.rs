#![allow(clippy::needless_range_loop)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use memmap2::MmapOptions;
use std::fs;
use std::io::{Read, Write};
use tempfile::tempdir;

fn create_test_files(size: usize) -> (tempfile::TempDir, Vec<std::path::PathBuf>) {
    let dir = tempdir().unwrap();
    let mut paths = Vec::new();

    for i in 0..3 {
        let path = dir.path().join(format!("file_{}.mkv", i));
        let mut file = fs::File::create(&path).unwrap();

        // Create test data with different patterns
        let mut data = vec![0u8; size];
        for j in 0..size {
            data[j] = match i {
                0 => {
                    if j % 3 == 0 {
                        1
                    } else {
                        0
                    }
                }
                1 => {
                    if j % 5 == 0 {
                        2
                    } else {
                        0
                    }
                }
                2 => {
                    if j % 7 == 0 {
                        4
                    } else {
                        0
                    }
                }
                _ => 0,
            };
        }

        file.write_all(&data).unwrap();
        paths.push(path);
    }

    (dir, paths)
}

fn bench_regular_io(c: &mut Criterion) {
    let (_dir, paths) = create_test_files(10 * 1024 * 1024); // 10MB files

    c.bench_function("regular_io_10mb", |b| {
        b.iter(|| {
            // Simulate regular I/O by reading files into buffers
            let mut buffers = Vec::new();
            for path in &paths {
                let mut file = fs::File::open(path).unwrap();
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer).unwrap();
                buffers.push(buffer);
            }
            black_box(buffers.len())
        })
    });
}

fn bench_mmap_io(c: &mut Criterion) {
    let (_dir, paths) = create_test_files(10 * 1024 * 1024); // 10MB files

    c.bench_function("mmap_io_10mb", |b| {
        b.iter(|| {
            // Simulate memory-mapped I/O
            let mut mmaps = Vec::new();
            for path in &paths {
                let file = fs::File::open(path).unwrap();
                let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
                mmaps.push(mmap);
            }
            black_box(mmaps.len())
        })
    });
}

fn bench_regular_io_small(c: &mut Criterion) {
    let (_dir, paths) = create_test_files(1024 * 1024); // 1MB files

    c.bench_function("regular_io_1mb", |b| {
        b.iter(|| {
            // Simulate regular I/O by reading files into buffers
            let mut buffers = Vec::new();
            for path in &paths {
                let mut file = fs::File::open(path).unwrap();
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer).unwrap();
                buffers.push(buffer);
            }
            black_box(buffers.len())
        })
    });
}

fn bench_mmap_io_small(c: &mut Criterion) {
    let (_dir, paths) = create_test_files(1024 * 1024); // 1MB files

    c.bench_function("mmap_io_1mb", |b| {
        b.iter(|| {
            // Simulate memory-mapped I/O
            let mut mmaps = Vec::new();
            for path in &paths {
                let file = fs::File::open(path).unwrap();
                let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
                mmaps.push(mmap);
            }
            black_box(mmaps.len())
        })
    });
}

criterion_group!(
    benches,
    bench_regular_io,
    bench_mmap_io,
    bench_regular_io_small,
    bench_mmap_io_small
);
criterion_main!(benches);
