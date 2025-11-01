use anyhow::{Context, Result};
use clap::Parser;
use memmap2::Mmap;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

const LARGE_FILE_THRESHOLD: u64 = 1024 * 1024;

const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Parser)]
#[command(name = "hasher")]
#[command(about = "Fast Merkle tree hashing for files and directories")]
struct Args {
    path: PathBuf,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    threads: Option<usize>,
}

#[derive(Debug)]
struct HashResult {
    path: PathBuf,
    hash: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(threads) = args.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .context("Failed to build thread pool")?;
    }

    let result = hash_path(&args.path, args.verbose)?;

    println!("{}", result.hash);

    Ok(())
}

fn hash_path(path: &Path, verbose: bool) -> Result<HashResult> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("Failed to read metadata for: {}", path.display()))?;

    if metadata.is_file() {
        let hash = if metadata.len() > LARGE_FILE_THRESHOLD {
            hash_large_file(path)?
        } else {
            hash_small_file(path)?
        };

        if verbose {
            println!("FILE {} -> {}", path.display(), hash);
        }

        Ok(HashResult {
            path: path.to_path_buf(),
            hash,
        })
    } else if metadata.is_dir() {
        hash_directory(path, verbose)
    } else {
        anyhow::bail!("Path is neither file nor directory: {}", path.display());
    }
}

fn hash_small_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}

fn hash_large_file(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;
    
    let mmap = unsafe {
        Mmap::map(&file)
            .with_context(|| format!("Failed to mmap file: {}", path.display()))?
    };

    let mut hasher = Sha256::new();
    
    for chunk in mmap.chunks(CHUNK_SIZE) {
        hasher.update(chunk);
    }
    
    Ok(hex::encode(hasher.finalize()))
}

fn hash_directory(path: &Path, verbose: bool) -> Result<HashResult> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory: {}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to enumerate directory: {}", path.display()))?;

    entries.sort_by_key(|e| e.file_name());

    let child_results: Result<Vec<HashResult>> = entries
        .par_iter()
        .map(|entry| hash_path(&entry.path(), verbose))
        .collect();

    let child_results = child_results?;

    let mut hasher = Sha256::new();
    for result in &child_results {
        let filename = result.path
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid filename")?;

        hasher.update(format!("{} {}\n", filename, result.hash).as_bytes());
    }

    let hash = hex::encode(hasher.finalize());

    if verbose {
        println!("DIR  {} -> {}", path.display(), hash);
    }

    Ok(HashResult {
        path: path.to_path_buf(),
        hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_small_file_hash() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, b"hello world").unwrap();

        let hash = hash_small_file(&file_path).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_directory_hash_deterministic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), b"content a").unwrap();
        fs::write(dir.path().join("b.txt"), b"content b").unwrap();

        let hash1 = hash_directory(dir.path(), false).unwrap().hash;
        let hash2 = hash_directory(dir.path(), false).unwrap().hash;

        assert_eq!(hash1, hash2, "Directory hash should be deterministic");
    }

    #[test]
    fn test_nested_directories() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        
        fs::write(dir.path().join("root.txt"), b"root").unwrap();
        fs::write(subdir.join("nested.txt"), b"nested").unwrap();

        let result = hash_path(dir.path(), false);
        assert!(result.is_ok());
    }
}
