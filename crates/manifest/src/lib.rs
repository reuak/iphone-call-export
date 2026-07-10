mod crypto;

use anyhow::{bail, Context, Result};
use plist::Value;
use std::{fs::File, io::Read, path::Path};

pub use crypto::verify_backup_password;

pub(crate) const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestStatus {
    pub size_bytes: u64,
    pub is_plain_sqlite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedBackupMetadata {
    pub version: Option<String>,
    pub is_encrypted: bool,
    pub backup_keybag: Vec<u8>,
    pub manifest_key_class: u32,
    pub wrapped_manifest_key: Vec<u8>,
    pub keybag_entries: Vec<KeybagEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybagEntry {
    pub tag: String,
    pub value: Vec<u8>,
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

pub fn read_encrypted_backup_metadata(backup_dir: &Path) -> Result<EncryptedBackupMetadata> {
    let path = backup_dir.join("Manifest.plist");
    let root = Value::from_file(&path)
        .with_context(|| format!("Manifest.plist kann nicht gelesen werden: {}", path.display()))?;
    let dict = root
        .as_dictionary()
        .context("Manifest.plist ist kein Dictionary")?;

    let is_encrypted = dict
        .get("IsEncrypted")
        .and_then(Value::as_boolean)
        .unwrap_or(false);
    if !is_encrypted {
        bail!("Das Backup ist laut Manifest.plist nicht verschlüsselt");
    }

    let backup_keybag = data_value(dict.get("BackupKeyBag"), "BackupKeyBag")?;
    let manifest_key = data_value(dict.get("ManifestKey"), "ManifestKey")?;
    if manifest_key.len() < 5 {
        bail!("ManifestKey ist zu kurz: {} Bytes", manifest_key.len());
    }

    let class_bytes: [u8; 4] = manifest_key[..4]
        .try_into()
        .expect("slice length was checked");
    let manifest_key_class = u32::from_le_bytes(class_bytes);
    let wrapped_manifest_key = manifest_key[4..].to_vec();
    let keybag_entries = parse_keybag_tlv(&backup_keybag)?;

    Ok(EncryptedBackupMetadata {
        version: dict
            .get("Version")
            .and_then(Value::as_string)
            .map(ToOwned::to_owned),
        is_encrypted,
        backup_keybag,
        manifest_key_class,
        wrapped_manifest_key,
        keybag_entries,
    })
}

pub fn parse_keybag_tlv(input: &[u8]) -> Result<Vec<KeybagEntry>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;

    while offset < input.len() {
        if input.len() - offset < 8 {
            bail!("Unvollständiger Keybag-TLV-Kopf bei Byte {offset}");
        }

        let tag_bytes: [u8; 4] = input[offset..offset + 4]
            .try_into()
            .expect("four-byte tag");
        let tag = String::from_utf8_lossy(&tag_bytes).into_owned();
        let length = u32::from_be_bytes(
            input[offset + 4..offset + 8]
                .try_into()
                .expect("four-byte length"),
        ) as usize;
        offset += 8;

        let end = offset
            .checked_add(length)
            .context("Keybag-TLV-Länge läuft über")?;
        if end > input.len() {
            bail!(
                "Keybag-TLV {tag:?} erwartet {length} Bytes, es sind aber nur {} vorhanden",
                input.len() - offset
            );
        }

        entries.push(KeybagEntry {
            tag,
            value: input[offset..end].to_vec(),
        });
        offset = end;
    }

    Ok(entries)
}

pub fn keybag_tag_u32(entries: &[KeybagEntry], tag: &str) -> Option<u32> {
    let value = entries.iter().find(|entry| entry.tag == tag)?.value.as_slice();
    let bytes: [u8; 4] = value.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn data_value(value: Option<&Value>, name: &str) -> Result<Vec<u8>> {
    match value {
        Some(Value::Data(data)) => Ok(data.clone()),
        Some(_) => bail!("{name} hat in Manifest.plist nicht den Datentyp Data"),
        None => bail!("{name} fehlt in Manifest.plist"),
    }
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

    #[test]
    fn parses_keybag_tlv_entries() {
        let bytes = [
            b'V', b'E', b'R', b'S', 0, 0, 0, 4, 0, 0, 0, 4,
            b'I', b'T', b'E', b'R', 0, 0, 0, 4, 0, 0, 0, 10,
        ];
        let entries = parse_keybag_tlv(&bytes).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(keybag_tag_u32(&entries, "VERS"), Some(4));
        assert_eq!(keybag_tag_u32(&entries, "ITER"), Some(10));
    }

    #[test]
    fn rejects_truncated_keybag_tlv() {
        let bytes = [b'S', b'A', b'L', b'T', 0, 0, 0, 8, 1, 2];
        let error = parse_keybag_tlv(&bytes).expect_err("must reject");
        assert!(error.to_string().contains("erwartet 8 Bytes"));
    }
}
