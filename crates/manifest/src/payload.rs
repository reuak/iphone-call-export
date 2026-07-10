use crate::SQLITE_HEADER;
use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
    Aes256,
};
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptedPayloadInfo {
    pub size_bytes: u64,
    pub header: [u8; 16],
    pub is_sqlite: bool,
}

pub fn decrypt_backup_payload(
    encrypted_path: &Path,
    output_path: &Path,
    file_key: &[u8],
) -> Result<DecryptedPayloadInfo> {
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
    let mut pending_plaintext: Option<[u8; 16]> = None;
    let mut written = 0_u64;

    loop {
        match reader.read_exact(&mut ciphertext) {
            Ok(()) => {
                let current_ciphertext = ciphertext;
                let mut block = GenericArray::clone_from_slice(&ciphertext);
                cipher.decrypt_block(&mut block);

                let mut plaintext = [0_u8; 16];
                for index in 0..16 {
                    plaintext[index] = block[index] ^ previous_ciphertext[index];
                }
                previous_ciphertext = current_ciphertext;

                if let Some(previous_plaintext) = pending_plaintext.replace(plaintext) {
                    writer.write_all(&previous_plaintext)?;
                    written += 16;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => {
                return Err(error).context("Backup-Datei konnte nicht vollständig gelesen werden")
            }
        }
    }

    let mut last = pending_plaintext.context("Backup-Datei enthält keinen vollständigen Block")?;
    let keep = pkcs7_unpadded_len(&last);
    writer.write_all(&last[..keep])?;
    written += keep as u64;
    last.fill(0);
    writer.flush()?;

    let mut check = File::open(output_path)?;
    let mut header = [0_u8; 16];
    check
        .read_exact(&mut header)
        .context("Entschlüsselter Dateiinhalt ist kürzer als 16 Bytes")?;

    Ok(DecryptedPayloadInfo {
        size_bytes: written,
        is_sqlite: &header == SQLITE_HEADER,
        header,
    })
}

fn pkcs7_unpadded_len(block: &[u8; 16]) -> usize {
    let padding = block[15] as usize;
    if padding == 0 || padding > 16 {
        return 16;
    }
    if block[16 - padding..]
        .iter()
        .all(|byte| *byte as usize == padding)
    {
        16 - padding
    } else {
        16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_valid_pkcs7_padding() {
        let mut block = [0_u8; 16];
        block[12..].fill(4);
        assert_eq!(pkcs7_unpadded_len(&block), 12);
        block[14] = 3;
        assert_eq!(pkcs7_unpadded_len(&block), 16);
    }
}
