use anyhow::{Context, Result};
use std::{fs::File, io::Read, path::Path};

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestStatus {
    pub size_bytes: u64,
    pub is_plain_sqlite: bool,
}

pub fn inspect_manifest_db(backup_dir: &Path) -> Result<ManifestStatus> {
    let path = backup_dir.join("Manifest.db");
    let metadata = path
        .metadata()
        .with_context(|| format!("Manifest.db fehlt oder kann nicht gelesen werden: {}", path.display()))?;

    let mut file = File::open(&path)
        .with_context(|| format!("Manifest.db kann nicht geöffnet werden: {}", path.display()))?;
    let mut header = [0_u8; 16];
    let bytes_read = file
        .read(&mut header)
        .with_context(|| format!("Manifest.db-Kopf kann nicht gelesen werden: {}", path.display()))?;

    Ok(ManifestStatus {
        size_bytes: metadata.len(),
        is_plain_sqlite: bytes_read == SQLITE_HEADER.len() && &header == SQLITE_HEADER,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("iphone-call-export-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn detects_plain_sqlite_header() {
        let dir = temp_dir("plain");
        fs::write(dir.join("Manifest.db"), b"SQLite format 3\0payload").expect("write");

        let status = inspect_manifest_db(&dir).expect("inspect");
        assert!(status.is_plain_sqlite);
        assert_eq!(status.size_bytes, 23);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_non_sqlite_header() {
        let dir = temp_dir("encrypted");
        fs::write(dir.join("Manifest.db"), [0x91_u8; 32]).expect("write");

        let status = inspect_manifest_db(&dir).expect("inspect");
        assert!(!status.is_plain_sqlite);
        assert_eq!(status.size_bytes, 32);

        let _ = fs::remove_dir_all(dir);
    }
}
