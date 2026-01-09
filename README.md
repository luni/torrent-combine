# Torrent Combine

A Rust CLI tool to merge partially downloaded torrent files (e.g., videos) within a directory tree. It groups files by name and size, performs sanity checks for compatibility, and merges them using bitwise OR on their contents. Merged files are saved with a `.merged` suffix or can replace originals with the `--replace` flag.

## Description

This tool scans a root directory recursively for files larger than 1MB (targeting video files). It assumes partial torrent downloads are pre-allocated with zeros and merges compatible files:

- **Grouping**: Files with identical basenames and sizes.
- **Sanity Check**: Non-zero bytes at each position must match across files.
- **Merge**: Bitwise OR of contents to combine downloaded chunks.
- **Output**: Creates `.merged` files for incomplete originals (unless `--replace` is used to overwrite them).
- Skips groups if all files are already complete or if sanity fails.

For details, see [DESIGN.md](DESIGN.md).

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

### Options

- `--replace`: Replace incomplete original files with merged content instead of creating `.merged` files.
- `--dry-run`: Show what would happen without actually modifying any files.
- `--extensions <EXT1,EXT2,...>`: Only process files with specified extensions (e.g., `mkv,mp4,avi`). Default: all files.
- `--dedup-mode <MODE>`: Deduplication mode. Options:
  - `filename-and-size`: Group files by filename and size (default)
  - `size-only`: Group files by size only
  - `extension-and-size`: Group files by extension and size
- `--no-mmap`: Disable memory mapping for file I/O (auto-enabled for files ≥ 5MB)

## Examples

Assume two partial files `/downloads/torrent-a/video.mkv` (size 10MB, partial) and `/downloads/torrent-b/video.mkv` (size 10MB, more complete):

### Basic Usage

```bash
torrent-combine /downloads
```

This creates `/downloads/torrent-a/video.mkv.merged` if the `torrent-a/video.mkv` was able to fill in missing chunks from `torrent-b/video.mkv`.

Likewise the `/downloads/torrent-b/video.mkv.merged` file is created if the `torrent-b/video.mkv` file was able to fill in missing chunks from `torrent-a/video.mkv`.

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

### Extension and Size Grouping

```bash
torrent-combine /downloads --dedup-mode extension-and-size
```

Groups files by extension and size instead of filename. Useful when files have different names but same content type.

### In-place Replacement

```bash
torrent-combine /downloads --replace
```

This overwrites the incomplete `/downloads/torrent-a/video.mkv` and or `/downloads/torrent-b/video.mkv` with the merged content if applicable.

### Memory Mapping (Performance)

```bash
torrent-combine /downloads
```

**Automatic optimization**: Memory mapping is automatically used for files ≥ 5MB for optimal performance.

```bash
torrent-combine /downloads --no-mmap
```

Disable memory mapping and use regular I/O for all files (useful for compatibility or debugging).

## Performance

Memory mapping provides substantial performance improvements for large files:

- **10MB files**: ~750x faster (15.6ms → 20.8µs)
- **1MB files**: ~8.5x faster (177µs → 20.8µs)

The tool automatically uses the optimal I/O method based on file size, with a 5MB threshold for memory mapping.

## Contributing

Fork the repo, make changes, and submit a pull request. See [CONVENTIONS.md](CONVENTIONS.md) for coding standards.

## License

MIT License.
