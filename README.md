# Torrent Combine

[![CI](https://github.com/mason-larobina/torrent-combine/workflows/test/badge.svg)](https://github.com/mason-larobina/torrent-combine/actions/workflows/test.yml)
[![Coverage](https://codecov.io/gh/mason-larobina/torrent-combine/branch/main/graph/badge.svg)](https://codecov.io/gh/mason-larobina/torrent-combine)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Crates.io](https://img.shields.io/crates/v/torrent-combine.svg)](https://crates.io/crates/torrent-combine)

A high-performance Rust CLI tool to merge partially downloaded torrent files (e.g., videos) within a directory tree. It groups files by name and size, performs sanity checks for compatibility, and merges them using bitwise OR on their contents. Features intelligent caching, progress bars, and robust error handling.

## Description

This tool scans a root directory recursively for files larger than 1MB (targeting video files). It assumes partial torrent downloads are pre-allocated with zeros and merges compatible files:

- **Grouping**: Files with identical basenames and sizes (or other deduplication modes)
- **Sanity Check**: Non-zero bytes at each position must match across files
- **Merge**: Bitwise OR of contents to combine downloaded chunks
- **Output**: Creates `.merged` files for incomplete originals (unless `--replace` is used)
- **Caching**: Intelligent file verification caching for faster subsequent runs
- **Progress Bars**: Real-time progress feedback for file scanning and processing
- **Error Handling**: Graceful handling of malformed paths, permission issues, and filesystem errors

For details, see [DESIGN.md](DESIGN.md).

## Features

- **🚀 High Performance**: Automatic memory mapping for files ≥ 5MB (23x faster with caching)
- **💾 Intelligent Caching**: Skip re-verification of unchanged files between runs
- **📊 Progress Bars**: Real-time progress for file discovery and group processing
- **🛡️ Robust Error Handling**: Graceful handling of malformed paths and permission issues
- **🔧 Multiple Deduplication Modes**: Group by filename+size, size-only, or extension+size
- **🧹 Clean Cleanup**: Automatic temporary file cleanup on success, failure, or cancellation
- **⚡ Parallel Processing**: Multi-threaded processing for faster execution
- **🎯 Dry Run Mode**: Preview operations without modifying files
- **📁 Extension Filtering**: Process only specific file types

## Installation

Both require Rust and Cargo (install via [rustup](https://rustup.rs/)).

### Via cargo install

```bash
cargo install torrent-combine
```

### From source

```bash
git clone https://github.com/mason-larobina/torrent-combine
cd torrent-combine
cargo install --path=.
```

## Usage

Run the tool with a root directory path:

```bash
torrent-combine /path/to/torrent/root/dir
```

## Options

### Core Options
- `--replace`: Replace incomplete original files with merged content instead of creating `.merged` files
- `--dry-run`: Show what would happen without actually modifying any files
- `--extensions <EXT1,EXT2,...>`: Only process files with specified extensions (e.g., `mkv,mp4,avi`). Default: all files
- `--dedup-mode <MODE>`: Deduplication mode:
  - `filename-and-size`: Group files by filename and size (default)
  - `size-only`: Group files by size only
  - `extension-and-size`: Group files by extension and size

### Performance Options
- `--no-mmap`: Disable memory mapping for file I/O (auto-enabled for files ≥ 5MB)
- `--no-cache`: Disable caching (slower but uses less disk space)
- `--clear-cache`: Clear cache before processing
- `--num-threads <N>`: Set number of processing threads (default: CPU count)

### Directory Options
- `<ROOT_DIRS>`: Root directories to search for files (positional arguments, required)
- `--src <DIR>`: Specify source directories to treat as read-only (can be used multiple times, files won't be modified)
- `--exclude <DIR>`: Exclude directories from scanning (can be used multiple times)
- `--min-file-size <SIZE>`: Minimum file size to process (e.g., `10MB`, `1GB`, `1048576'). Default: 1MB

### Output Options
- `--verbose`: Enable verbose logging (may interfere with progress bar)

## Examples

### Basic Usage

```bash
# Single directory (files can be both source and target)
torrent-combine /downloads

# Multiple directories (files can be both source and target)
torrent-combine /downloads1 /downloads2 /downloads3

# Multiple directories with separate read-only source directories
torrent-combine /downloads1 /downloads2 --src /readonly/torrents --src /backup/torrents

# Multiple directories with exclusions
torrent-combine /downloads1 /downloads2 --exclude /temp --exclude /cache

# Combined with other options
torrent-combine /downloads1 /downloads2 --src /readonly/torrents --exclude /temp --extensions mkv --min-size 10MB
```

**Behavior:**
- **Without `--src`**: Files in root directories can be both read from and written to
- **With `--src`**: Files in `--src` directories are read-only, files in root directories are targets

This creates `/downloads/torrent-a/video.mkv.merged` if the `torrent-a/video.mkv` was able to fill in missing chunks from `torrent-b/video.mkv`.

### Dry Run Mode

```bash
torrent-combine /downloads --dry-run
```

Shows what would happen without actually modifying any files.

### Extension Filtering

```bash
torrent-combine /downloads --extensions mkv,mp4,avi
```

Only processes files with `.mkv`, `.mp4`, or `.avi` extensions.

### Size-Based Filtering

```bash
torrent-combine /downloads --min-file-size 10MB
```

Only processes files larger than 10MB.

### Different Deduplication Modes

```bash
# Group by extension and size (useful for files with different names)
torrent-combine /downloads --dedup-mode extension-and-size

# Group by size only (useful for identical files with different names)
torrent-combine /downloads --dedup-mode size-only
```

### Source Directory Control

```bash
# Single source directory
torrent-combine /downloads --src /readonly/torrents

# Multiple source directories (can be used multiple times)
torrent-combine /downloads --src /readonly/torrents --src /backup/torrents --src /archive/torrents

# Exclude directories from scanning
torrent-combine /downloads --exclude /downloads/temp --exclude /downloads/incomplete

# Combine source and exclude options
torrent-combine /downloads --src /readonly/torrents --exclude /downloads/temp --exclude /downloads/cache
```

Treat files in specified directories as read-only sources (won't be modified). This is useful when you have completed downloads in one location and want to use them as sources to fix incomplete downloads in another location.

The `--exclude` option prevents scanning of specified directories and all their subdirectories, which is useful for skipping temporary files, cache directories, or incomplete downloads.

### In-place Replacement

```bash
torrent-combine /downloads --replace
```

This overwrites the incomplete files with merged content instead of creating `.merged` files.

### Performance Optimization

```bash
# Automatic optimization (default)
torrent-combine /downloads

# Disable memory mapping (for compatibility/debugging)
torrent-combine /downloads --no-mmap

# Disable caching (saves disk space)
torrent-combine /downloads --no-cache

# Clear existing cache
torrent-combine /downloads --clear-cache
```

### Verbose Output

```bash
torrent-combine /downloads --verbose
```

Shows detailed processing information (may interfere with progress bar display).

## Performance

### Memory Mapping Benefits

Memory mapping provides substantial performance improvements for large files:

- **10MB files**: ~750x faster (15.6ms → 20.8µs)
- **1MB files**: ~8.5x faster (177µs → 20.8µs)

The tool automatically uses the optimal I/O method based on file size, with a 5MB threshold for memory mapping.

### Caching Performance

Intelligent caching dramatically speeds up subsequent runs:

- **First run**: Full file verification and processing
- **Second run**: ~23x faster (1.369s → 0.060s) when files haven't changed

Cache automatically detects file modifications and only reprocesses changed files.

## Cache Management

The tool automatically creates and manages a cache in `.torrent-combine-cache/`:

- **File metadata caching**: Stores file sizes, modification times, and hashes
- **Group result caching**: Stores processing results for each file group
- **Automatic cleanup**: Cache entries expire after 1 hour
- **Change detection**: Automatically invalidates cache when files are modified

### Cache Control

```bash
# Clear cache manually
torrent-combine /downloads --clear-cache

# Disable caching entirely
torrent-combine /downloads --no-cache

# Check cache status (verbose mode)
torrent-combine /downloads --verbose | grep "cached"
```

## Error Handling

The tool gracefully handles various error conditions:

- **Malformed paths**: Skips files with invalid characters (null bytes, extremely long names)
- **Permission issues**: Continues processing other files when access is denied
- **Filesystem errors**: Logs warnings and continues with other files
- **Temporary file cleanup**: Automatically cleans up `.tmp` files on success, failure, or cancellation

## Progress Indicators

The tool provides clear progress feedback:

```
⠁ Scanning for large files...
File scanning complete
⠁ [00:00:03] [####################] 15/20 (ETA: 0s) Processing groups
Processing complete
```

- **Discovery phase**: Spinner animation while scanning directories
- **Processing phase**: Progress bar with current/total count and ETA
- **Cache hits**: Shows "cached" messages when using cached results

## Contributing

Fork the repo, make changes, and submit a pull request. See [CONVENTIONS.md](CONVENTIONS.md) for coding standards.

### CI/CD

This project uses GitHub Actions for continuous integration and deployment:

- **Automated Testing**: All PRs and pushes to `main` trigger comprehensive test suites
- **Cross-Platform Testing**: Tests run on Ubuntu, macOS, and Windows
- **Security Auditing**: Automated vulnerability scanning with `cargo audit`
- **Performance Benchmarks**: Performance tracking on every push to `main`
- **Code Coverage**: Coverage reporting with Codecov integration
- **Release Automation**: Automatic releases when tags are pushed

### Development Workflow

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Ensure all tests pass (`cargo test`)
5. Run formatting checks (`cargo fmt --check`)
6. Run clippy (`cargo clippy`)
7. Submit a pull request

The CI system will automatically:
- Run the full test suite across multiple platforms
- Check code formatting and linting
- Perform security audits
- Generate coverage reports
- Run performance benchmarks

## License

MIT License.
