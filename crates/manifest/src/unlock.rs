use crate::{EncryptedBackupMetadata, FileEncryptionMetadata, KeybagEntry, SQLITE_HEADER};
use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
    Aes256,
};
use anyhow::{bail, Context, Result};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use sha2::Sha256;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

const WRAP_PASSCODE: u32 = 2;
const RFC3394_IV: [u8; 8] = [0xA6; 8];

#[derive(Debug)]
pub struct UnlockedBackup {
    manifest_key: Zeroizing<Vec<u8>>,
    class_keys: HashMap<u32, Zeroizing<Vec<u8>>>,
}

impl UnlockedBackup {
    pub fn manifest_key(&self) -> &[u8] {
        &self.manifest_key
    }

    pub fn unlock_file_key(
        &self,
        metadata: &FileEncryptionMetadata,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let class_key = self
            .class_keys
            .get(&metadata.protection_class)
            .with_context(|| {
                format!(
                    "Schlüsselklasse {} wurde nicht entsperrt",
                    metadata.protection_class
                )
            })?;
        let file_key = aes_key_unwrap(class_key, &metadata.wrapped_key)
            .context("Dateischlüssel konnte nicht entsperrt werden")?;
        if file_key.len() != 32 {
            bail!(
                "Entsperrter Dateischlüssel hat {} statt 32 Bytes",
                file_key.len()
            );
        }
        Ok(Zeroizing::new(file_key))
    }
}

pub fn unlock_backup(
    metadata: &EncryptedBackupMetadata,
    password: &[u8],
) -> Result<Option<UnlockedBackup>> {
    let passcode_key = derive_passcode_key(&metadata.keybag_entries, password)?;
    let records = parse_class_keys(&metadata.keybag_entries)?;
    let mut class_keys = HashMap::new();

    for record in records {
        if record.wrap & WRAP_PASSCODE == 0 {
            continue;
        }
        let key = match aes_key_unwrap(&passcode_key, &record.wrapped_key) {
            Ok(key) => key,
            Err(_) => return Ok(None),
        };
        if key.len() != 32 {
            bail!(
                "Entsperrter Klassenschlüssel {} hat {} statt 32 Bytes",
                record.class,
                key.len()
            );
        }
        class_keys.insert(record.class, Zeroizing::new(key));
    }

    let manifest_class_key = class_keys
        .get(&metadata.manifest_key_class)
        .with_context(|| {
            format!(
                "Manifest-Schlüsselklasse {} wurde nicht entsperrt",
                metadata.manifest_key_class
            )
        })?;
    let manifest_key = match aes_key_unwrap(manifest_class_key, &metadata.wrapped_manifest_key) {
        Ok(key) => key,
        Err(_) => return Ok(None),
    };
    if manifest_key.len() != 32 {
        bail!(
            "Entsperrter Manifest-Schlüssel hat {} statt 32 Bytes",
            manifest_key.len()
        );
    }

    Ok(Some(UnlockedBackup {
        manifest_key: Zeroizing::new(manifest_key),
        class_keys,
    }))
}

/// Decrypts an encrypted backup file using AES-256-CBC with a zero IV.
/// The encrypted file is block padded; the exact logical size from Manifest.db
/// is used to truncate the output without guessing a padding format.
pub fn decrypt_backup_file(
    encrypted_path: &Path,
    output_path: &Path,
    file_key: &[u8],
    logical_size: u64,
) -> Result<u64> {
    if file_key.len() != 32 {
        bail!("Dateischlüssel muss 32 Bytes lang sein");
    }
    let input = File::open(encrypted_path).with_context(|| {
        format!(
            "Verschlüsselte Backup-Datei kann nicht geöffnet werden: {}",
            encrypted_path.display()
        )
    })?;
    let encrypted_size = input.metadata()?.len();
    if encrypted_size == 0 || encrypted_size % 16 != 0 {
        bail!(
            "Verschlüsselte Backup-Datei hat keine gültige AES-Blocklänge: {encrypted_size} Bytes"
        );
    }
    if logical_size > encrypted_size {
        bail!(
            "Logische Dateigröße {logical_size} ist größer als die verschlüsselte Datei {encrypted_size}"
        );
    }

    let cipher = Aes256::new_from_slice(file_key)
        .context("Datei-AES-256 konnte nicht initialisiert werden")?;
    let mut reader = BufReader::new(input);
    let output = File::create(output_path).with_context(|| {
        format!(
            "Temporäre entschlüsselte Datei kann nicht erstellt werden: {}",
            output_path.display()
        )
    })?;
    let mut writer = BufWriter::new(output);
    let mut previous_ciphertext = [0_u8; 16];
    let mut ciphertext = [0_u8; 16];
    let mut written = 0_u64;

    while written < logical_size {
        reader
            .read_exact(&mut ciphertext)
            .context("Verschlüsselte Backup-Datei endet unerwartet")?;
        let current_ciphertext = ciphertext;
        let mut block = GenericArray::clone_from_slice(&ciphertext);
        cipher.decrypt_block(&mut block);

        let mut plaintext = [0_u8; 16];
        for index in 0..16 {
            plaintext[index] = block[index] ^ previous_ciphertext[index];
        }
        previous_ciphertext = current_ciphertext;

        let remaining = (logical_size - written) as usize;
        let keep = remaining.min(16);
        writer.write_all(&plaintext[..keep])?;
        plaintext.fill(0);
        written += keep as u64;
    }
    writer.flush()?;

    let mut check = File::open(output_path)?;
    let mut header = [0_u8; 16];
    check.read_exact(&mut header)?;
    if &header != SQLITE_HEADER {
        bail!("Entschlüsselte CallHistory.storedata besitzt keinen gültigen SQLite-Kopf");
    }
    Ok(written)
}

#[derive(Debug)]
struct ClassKeyRecord {
    class: u32,
    wrap: u32,
    wrapped_key: Vec<u8>,
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
        match entry.tag.as_str() {
            "UUID" => {
                if let (Some(class), Some(wrap), Some(wrapped_key)) =
                    (class.take(), wrap.take(), wrapped_key.take())
                {
                    records.push(ClassKeyRecord { class, wrap, wrapped_key });
                }
                class = None;
                wrap = None;
                wrapped_key = None;
            }
            "CLAS" => class = parse_u32(&entry.value),
            "WRAP" => wrap = parse_u32(&entry.value),
            "WPKY" => wrapped_key = Some(entry.value.clone()),
            _ => {}
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
    if kek.len() != 32 || wrapped.len() < 24 || wrapped.len() % 8 != 0 {
        bail!("Ungültige RFC3394-Schlüssellänge");
    }
    let cipher = Aes256::new_from_slice(kek).context("AES-256 konnte nicht initialisiert werden")?;
    let n = wrapped.len() / 8 - 1;
    let mut a: [u8; 8] = wrapped[..8].try_into().expect("eight bytes");
    let mut r: Vec<[u8; 8]> = wrapped[8..]
        .chunks_exact(8)
        .map(|chunk| chunk.try_into().expect("eight bytes"))
        .collect();
    for j in (0_u64..=5).rev() {
        for i in (1..=n).rev() {
            let t = n as u64 * j + i as u64;
            let t_bytes = t.to_be_bytes();
            let mut block = [0_u8; 16];
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
    Ok(r.into_iter().flatten().collect())
}

fn required_tag<'a>(entries: &'a [KeybagEntry], tag: &str) -> Result<&'a [u8]> {
    entries
        .iter()
        .find(|entry| entry.tag == tag)
        .map(|entry| entry.value.as_slice())
        .with_context(|| format!("Keybag-Tag {tag} fehlt"))
}

fn required_u32(entries: &[KeybagEntry], tag: &str) -> Result<u32> {
    parse_u32(required_tag(entries, tag)?)
        .with_context(|| format!("Keybag-Tag {tag} ist kein u32"))
}

fn parse_u32(value: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(value.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_logical_size_larger_than_ciphertext() {
        let dir = std::env::temp_dir();
        let input = dir.join("iphone-call-export-short-cipher.bin");
        let output = dir.join("iphone-call-export-short-plain.bin");
        std::fs::write(&input, [0_u8; 16]).expect("write");
        let error = decrypt_backup_file(&input, &output, &[0_u8; 32], 17)
            .expect_err("must reject");
        assert!(error.to_string().contains("größer"));
        let _ = std::fs::remove_file(input);
        let _ = std::fs::remove_file(output);
    }
}
