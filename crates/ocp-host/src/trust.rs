use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrustError {
    #[error("failed to read binary for hashing at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn compute_sha256(path: &Path) -> Result<String, TrustError> {
    let mut file = fs::File::open(path).map_err(|source| TrustError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(|source| TrustError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Compute SHA-256 of raw bytes using sha2 directly; used as the expected value.
    fn sha256_bytes(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    #[test]
    fn matches_direct_sha2_computation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        let content = b"hello world from open-choice";
        std::fs::write(&path, content).unwrap();
        assert_eq!(compute_sha256(&path).unwrap(), sha256_bytes(content));
    }

    #[test]
    fn empty_file_matches_sha2_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.bin");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(compute_sha256(&path).unwrap(), sha256_bytes(b""));
    }

    #[test]
    fn large_file_exercises_read_buffer_loop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        // 20 KiB — larger than the 8192-byte read buffer so multiple iterations run
        let data = vec![0xABu8; 20 * 1024];
        std::fs::write(&path, &data).unwrap();
        assert_eq!(compute_sha256(&path).unwrap(), sha256_bytes(&data));
    }

    #[test]
    fn output_is_lowercase_hex_64_chars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.bin");
        std::fs::write(&path, b"test").unwrap();
        let hash = compute_sha256(&path).unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn different_content_produces_different_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("a.bin");
        let p2 = dir.path().join("b.bin");
        std::fs::write(&p1, b"content-one").unwrap();
        std::fs::write(&p2, b"content-two").unwrap();
        assert_ne!(compute_sha256(&p1).unwrap(), compute_sha256(&p2).unwrap());
    }

    #[test]
    fn missing_file_returns_io_error() {
        let result = compute_sha256(std::path::Path::new(
            "/nonexistent/path/that/cannot/exist/file.bin",
        ));
        assert!(matches!(result, Err(TrustError::Io { .. })));
    }
}
