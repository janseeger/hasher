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
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to read metadata for: {}", path.display()))?;

    if metadata.is_symlink() {
        let hash = hash_symlink(path)?;

        if verbose {
            println!("LINK {} -> {}", path.display(), hash);
        }

        Ok(HashResult {
            path: path.to_path_buf(),
            hash,
        })
    } else if metadata.is_file() {
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
        let child_results = hash_directory(path, verbose)?;
        
        let child_hashes: Vec<(String, String)> = child_results
            .iter()
            .map(|result| {
                let filename = result.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .context("Invalid filename")?;
                Ok((filename.to_string(), result.hash.clone()))
            })
            .collect::<Result<Vec<_>>>()?;

        let hash = build_merkle_hash(&child_hashes);

        if verbose {
            println!("DIR  {} -> {}", path.display(), hash);
        }

        Ok(HashResult {
            path: path.to_path_buf(),
            hash,
        })
    } else {
        anyhow::bail!("Path is neither file nor directory: {}", path.display());
    }
}

fn hash_symlink(path: &Path) -> Result<String> {
    let target = fs::read_link(path)
        .with_context(|| format!("Failed to read symlink: {}", path.display()))?;
    
    let target_str = target.to_str()
        .context("Symlink target contains invalid UTF-8")?;
    
    let hash = Sha256::digest(target_str.as_bytes());
    Ok(hex::encode(hash))
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

fn hash_directory(path: &Path, verbose: bool) -> Result<Vec<HashResult>> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory: {}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to enumerate directory: {}", path.display()))?;

    entries.sort_by_key(|e| e.file_name());

    let threshold = rayon::current_num_threads();
    if entries.len() > threshold {
        entries
            .into_par_iter()
            .map(|entry| hash_path(&entry.path(), verbose))
            .collect()
    } else {
        entries
            .into_iter()
            .map(|entry| hash_path(&entry.path(), verbose))
            .collect()
    }
}

pub fn build_merkle_hash(entries: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (filename, hash) in entries {
        hasher.update(format!("{} {}\n", filename, hash).as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::os::unix::fs::symlink;

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
    fn test_merkle_hash_simple() {
        let entries = vec![
            ("a.txt".to_string(), "hash_a".to_string()),
            ("b.txt".to_string(), "hash_b".to_string()),
        ];

        let hash1 = build_merkle_hash(&entries);
        let hash2 = build_merkle_hash(&entries);

        assert_eq!(hash1, hash2, "Merkle hash should be deterministic");
    }

    #[test]
    fn test_merkle_hash_order_matters() {
        let entries1 = vec![
            ("a.txt".to_string(), "hash_a".to_string()),
            ("b.txt".to_string(), "hash_b".to_string()),
        ];

        let entries2 = vec![
            ("b.txt".to_string(), "hash_b".to_string()),
            ("a.txt".to_string(), "hash_a".to_string()),
        ];

        let hash1 = build_merkle_hash(&entries1);
        let hash2 = build_merkle_hash(&entries2);

        assert_ne!(hash1, hash2, "Merkle hash should depend on order");
    }

    #[test]
    fn test_directory_hash_deterministic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), b"content a").unwrap();
        fs::write(dir.path().join("b.txt"), b"content b").unwrap();

        let hash1 = hash_path(dir.path(), false).unwrap().hash;
        let hash2 = hash_path(dir.path(), false).unwrap().hash;

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

    #[test]
    #[cfg(unix)]
    fn test_symlink_hash() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        
        fs::write(&target, b"target content").unwrap();
        symlink("target.txt", &link).unwrap();

        let result = hash_path(&link, false);
        assert!(result.is_ok());

        let hash1 = hash_symlink(&link).unwrap();
        let hash2 = hash_symlink(&link).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    #[cfg(unix)]
    fn test_directory_with_symlink() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        
        fs::write(&target, b"content").unwrap();
        symlink("target.txt", &link).unwrap();

        let result = hash_path(dir.path(), false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_merkle_hash_with_real_file_hashes() {
        let dir = TempDir::new().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        
        fs::write(&file_a, b"content a").unwrap();
        fs::write(&file_b, b"content b").unwrap();

        let hash_a = hash_small_file(&file_a).unwrap();
        let hash_b = hash_small_file(&file_b).unwrap();

        let entries = vec![
            ("a.txt".to_string(), hash_a),
            ("b.txt".to_string(), hash_b),
        ];

        let merkle_hash = build_merkle_hash(&entries);
        
        let dir_hash = hash_path(dir.path(), false).unwrap().hash;
        assert_eq!(merkle_hash, dir_hash);
    }

    #[test]
    fn test_subdirectory_hash_consistency() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        
        fs::write(subdir.join("nested.txt"), b"nested content").unwrap();

        let subdir_hash = hash_path(&subdir, false).unwrap().hash;
        
        let parent_children = hash_directory(dir.path(), false).unwrap();
        let subdir_entry = parent_children
            .iter()
            .find(|r| r.path.file_name().unwrap() == "subdir")
            .unwrap();
        
        assert_eq!(subdir_hash, subdir_entry.hash);
    }
}