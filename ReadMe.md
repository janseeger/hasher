# Hasher

[![Build Status](https://github.com/janseeger/hasher/actions/workflows/rust.yml/badge.svg)](https://github.com/janseeger/hasher/actions/workflows/rust.yml)
[![GitHub License](https://img.shields.io/github/license/janseeger/hasher)](https://github.com/janseeger/hasher/blob/main/LICENSE)

A high-performance file and directory hashing tool written in Rust that creates Merkle tree hashes (similar to git's content-addressable storage).

## Features

- **Optimized for different file sizes**:
  - Small files (<1MB): Single read into memory
  - Large files (>1MB): Memory-mapped I/O with zero-copy
  - Directories: Recursive Merkle tree hashing
  - Symlinks: Hashes the link target path (not the target content)

- **Parallel processing**: Uses Rayon for efficient multi-threaded directory traversal

- **Deterministic**: Same content always produces the same hash (sorted directory entries)

- **Fast**: Memory-mapped I/O and parallel processing for maximum performance

## Installation

```bash
cargo build --release
```

The optimized binary will be at `target/release/hasher`

## Usage

Basic usage:
```bash
./hasher /path/to/directory
```

Show individual file/directory hashes:
```bash
./hasher -v /path/to/directory
```

Specify number of threads:
```bash
./hasher -t 8 /path/to/directory
```

Hash a single file:
```bash
./hasher /path/to/file.txt
```

## How It Works

1. **Files**: Hashed using SHA-256
   - Small files are read entirely into memory
   - Large files use memory-mapped I/O for efficiency

2. **Symlinks**: Hashed based on their target path
   - The symlink target path (relative or absolute) is hashed as text
   - Does not follow the symlink or hash the target's content
   - Broken symlinks are handled gracefully (only the path is hashed)

3. **Directories**: Creates a Merkle tree hash
   - Recursively hashes all children (files, subdirectories, and symlinks)
   - Combines child hashes in sorted order: `filename1 hash1\nfilename2 hash2\n...`
   - Hashes the combined string to produce directory hash

This ensures:
- Same content → same hash
- Any change propagates up the tree
- Efficient verification of large directory structures
- Symlinks are treated as independent entities based on their target path

## Performance

On a typical mixed workload (small and large files):
- ~2-5 GB/s for large files (using mmap)
- ~1-2 GB/s for small files
- Scales linearly with CPU cores for parallel directory traversal

## Cross-Compilation

For Linux ARM64:
```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

For Windows:
```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

For macOS ARM (M1/M2):
```bash
rustup target add aarch64-apple-darwin
cargo build --release --target aarch64-apple-darwin
```

## Testing

Run the test suite:
```bash
cargo test
```

## Example Output

```
$ ./hasher -v /my/project
FILE /my/project/src/main.rs -> a1b2c3d4...
FILE /my/project/src/lib.rs -> e5f6g7h8...
DIR  /my/project/src -> 1a2b3c4d...
LINK /my/project/config.link -> 3e4f5g6h...
FILE /my/project/Cargo.toml -> 9i0j1k2l...
DIR  /my/project -> 5m6n7o8p...
Root hash: 5m6n7o8p9q0r1s2t3u4v5w6x7y8z9a0b1c2d3e4f5g6h7i8j9k0l1m2n3o4p5q6r7s8t9u
Completed in 1.23s
```

## License

MIT
