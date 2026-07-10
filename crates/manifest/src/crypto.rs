use crate::{EncryptedBackupMetadata, KeybagEntry, SQLITE_HEADER};
use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
    Aes256,
};
use anyhow::{bail, Context, Result};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use sha2::Sha256;
use std::{fs::File, io::Read, path::Path};
use zeroize::Zeroizing;

const WRAP_PASSCODE: u32 = 2;
const RFC3394_IV: [u8; 8] = [0xA6; 8];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClassKeyRecord {
    class: u32,
    wrap: u32,
    wrapped_key: Vec<u8>,
}

pub fn verify_backup_password(
    backup_dir: &Path,
    metadata: &EncryptedBackupMetadata,
    password: &[u8],
) -> Result<bool> {
    let passcode_key = derive_passcode_key(&metadata.keybag_entries, password)?;
    let class_keys = parse_class_keys(&metadata.keybag_entries)?;
    let class_record = class_keys
        .iter()
        .find(|record| record.class == metadata.manifest_key_class)
        .with_context(|| {
            format!(
                "Keine Schlüsselklasse {} im BackupKeyBag gefunden",
                metadata.manifest_key_class
            )
        })?;

    if class_record.wrap & WRAP_PASSCODE == 0 {
        bail!(
            "Schlüsselklasse {} ist nicht mit dem Backup-Passwort geschützt (WRAP={})",
            class_record.class,
            class_record.wrap
        );
    }

    let class_key = match aes_key_unwrap(&passcode_key, &class_record.wrapped_key) {
        Ok(key) => Zeroizing::new(key),
        Err(_) => return Ok(false),
    };
    if class_key.len() != 32 {
        bail!("Entsperrter Klassenschlüssel hat {} statt 32 Bytes", class_key.len());
    }

    let manifest_key = match aes_key_unwrap(&class_key, &metadata.wrapped_manifest_key) {
        Ok(key) => Zeroizing::new(key),
        Err(_) => return Ok(false),
    };
    if manifest_key.len() != 32 {
        bail!("Entsperrter Manifest-Schlüssel hat {} statt 32 Bytes", manifest_key.len());
    }

    decrypt_manifest_header(backup_dir, &manifest_key)
}

fn derive_passcode_key(entries: &[KeybagEntry], password: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let salt = required_tag(entries, "SALT")?;
    let iter = required_u32(entries, "ITER")?;
    let dpsl = required_tag(entries, "DPSL")?;
    let dpic = required_u32(entries, "DPIC")?;

    let mut intermediate = Zeroizing::new(vec![0_u8; 32]);
    pbkdf2_hmac::<Sha256>(password, dpsl, dpic, &mut intermediate);

    let mut passcode_key = Zeroizing::new(vec![0_u8; 32]);
    pbkdf2_hmac::<Sha1>(&intermediate, salt, iter, &mut passcode_key);
    Ok(passcode_key)
}

fn parse_class_keys(entries: &[KeybagEntry]) -> Result<Vec<ClassKeyRecord>> {
    let mut records = Vec::new();
    let mut class = None;
    let mut wrap = None;
    let mut wrapped_key = None;

    for entry in entries {
        if entry.tag == "UUID" {
            if let (Some(class), Some(wrap), Some(wrapped_key)) =
                (class.take(), wrap.take(), wrapped_key.take())
            {
                records.push(ClassKeyRecord { class, wrap, wrapped_key });
            }
            class = None;
            wrap = None;
            wrapped_key = None;
        } else if entry.tag == "CLAS" {
            class = parse_u32_value(&entry.value);
        } else if entry.tag == "WRAP" {
            wrap = parse_u32_value(&entry.value);
        } else if entry.tag == "WPKY" {
            wrapped_key = Some(entry.value.clone());
        }
    }

    if let (Some(class), Some(wrap), Some(wrapped_key)) = (class, wrap, wrapped_key) {
        records.push(ClassKeyRecord { class, wrap, wrapped_key });
    }

    if records.is_empty() {
        bail!("Keine vollständigen Klassenschlüssel im BackupKeyBag gefunden");
    }
    Ok(records)
}

fn aes_key_unwrap(kek: &[u8], wrapped: &[u8]) -> Result<Vec<u8>> {
    if kek.len() != 32 {
        bail!("AES-Key-Encryption-Key muss 32 Bytes lang sein");
    }
    if wrapped.len() < 24 || wrapped.len() % 8 != 0 {
        bail!("RFC3394-Datenlänge ist ungültig: {} Bytes", wrapped.len());
    }

    let cipher = Aes256::new_from_slice(kek).context("AES-256 konnte nicht initialisiert werden")?;
    let n = wrapped.len() / 8 - 1;
    let mut a: [u8; 8] = wrapped[..8].try_into().expect("8-byte slice");
    let mut r: Vec<[u8; 8]> = wrapped[8..]
        .chunks_exact(8)
        .map(|chunk| chunk.try_into().expect("8-byte chunk"))
        .collect();

    for j in (0_u64..=5).rev() {
        for i in (1..=n).rev() {
            let t = n as u64 * j + i as u64;
            let mut block = [0_u8; 16];
            let t_bytes = t.to_be_bytes();
            for index in 0..8 {
                block[index] = a[index] ^ t_bytes[index];
            }
            block[8..].copy_from_slice(&r[i - 1]);

            let mut ga = GenericArray::clone_from_slice(&block);
            cipher.decrypt_block(&mut ga);
            a.copy_from_slice(&ga[..8]);
            r[i - 1].copy_from_slice(&ga[8..]);
        }
    }

    if a != RFC3394_IV {
        bail!("RFC3394-Integritätsprüfung fehlgeschlagen");
    }

    let mut output = Vec::with_capacity(n * 8);
    for block in r {
        output.extend_from_slice(&block);
    }
    Ok(output)
}

fn decrypt_manifest_header(backup_dir: &Path, manifest_key: &[u8]) -> Result<bool> {
    let path = backup_dir.join("Manifest.db");
    let mut file = File::open(&path)
        .with_context(|| format!("Manifest.db kann nicht geöffnet werden: {}", path.display()))?;
    let mut ciphertext = [0_u8; 16];
    file.read_exact(&mut ciphertext)
        .with_context(|| format!("Manifest.db-Kopf kann nicht gelesen werden: {}", path.display()))?;

    let cipher = Aes256::new_from_slice(manifest_key)
        .context("Manifest-AES-256 konnte nicht initialisiert werden")?;
    let mut block = GenericArray::clone_from_slice(&ciphertext);
    cipher.decrypt_block(&mut block);
    Ok(block.as_slice() == SQLITE_HEADER)
}

fn required_tag<'a>(entries: &'a [KeybagEntry], tag: &str) -> Result<&'a [u8]> {
    entries
        .iter()
        .find(|entry| entry.tag == tag)
        .map(|entry| entry.value.as_slice())
        .with_context(|| format!("Keybag-Tag {tag} fehlt"))
}

fn required_u32(entries: &[KeybagEntry], tag: &str) -> Result<u32> {
    let value = required_tag(entries, tag)?;
    parse_u32_value(value).with_context(|| format!("Keybag-Tag {tag} ist kein u32"))
}

fn parse_u32_value(value: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = value.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3394_rejects_invalid_integrity_value() {
        let kek = [0_u8; 32];
        let wrapped = [0_u8; 40];
        assert!(aes_key_unwrap(&kek, &wrapped).is_err());
    }

    #[test]
    fn parses_class_key_records() {
        let entries = vec![
            KeybagEntry { tag: "UUID".into(), value: vec![0; 16] },
            KeybagEntry { tag: "CLAS".into(), value: 3_u32.to_be_bytes().to_vec() },
            KeybagEntry { tag: "WRAP".into(), value: 2_u32.to_be_bytes().to_vec() },
            KeybagEntry { tag: "WPKY".into(), value: vec![1; 40] },
        ];
        let records = parse_class_keys(&entries).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].class, 3);
        assert_eq!(records[0].wrap, 2);
        assert_eq!(records[0].wrapped_key.len(), 40);
    }
}
