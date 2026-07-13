use anyhow::{bail, Context, Result};
use plist::Value;
use rusqlite::{Connection, OptionalExtension};
use std::{io::Cursor, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFileRecord {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub metadata_blob: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPathRecord {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub flags: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEncryptionMetadata {
    pub protection_class: u32,
    pub wrapped_key: Vec<u8>,
    pub logical_size: Option<u64>,
    pub size_before_copy: Option<u64>,
    pub content_compression_method: Option<u64>,
    pub content_encoding_method: Option<u64>,
}

pub fn find_call_history_record(database_path: &Path) -> Result<Option<ManifestFileRecord>> {
    let connection = Connection::open(database_path)
        .with_context(|| format!("Entschlüsselte Manifest.db kann nicht geöffnet werden: {}", database_path.display()))?;

    let mut statement = connection.prepare(
        "SELECT fileID, domain, relativePath, file\n\
         FROM Files\n\
         WHERE relativePath = 'Library/CallHistoryDB/CallHistory.storedata'\n\
            OR relativePath LIKE '%/CallHistory.storedata'\n\
         ORDER BY CASE WHEN domain = 'HomeDomain' THEN 0 ELSE 1 END\n\
         LIMIT 1",
    )?;

    statement
        .query_row([], |row| {
            Ok(ManifestFileRecord {
                file_id: row.get(0)?,
                domain: row.get(1)?,
                relative_path: row.get(2)?,
                metadata_blob: row.get(3)?,
            })
        })
        .optional()
        .context("CallHistory.storedata konnte in Manifest.db nicht gesucht werden")
}

/// Finds actual contact databases and vCard files, excluding preferences and UI assets.
pub fn find_contact_candidates(database_path: &Path) -> Result<Vec<ManifestPathRecord>> {
    let connection = Connection::open(database_path)
        .with_context(|| format!("Entschlüsselte Manifest.db kann nicht geöffnet werden: {}", database_path.display()))?;

    let mut statement = connection.prepare(
        "SELECT fileID, domain, relativePath, flags\n\
         FROM Files\n\
         WHERE lower(relativePath) LIKE '%.vcf'\n\
            OR lower(relativePath) LIKE '%addressbook%.sqlitedb'\n\
            OR lower(relativePath) LIKE '%addressbook%.sqlite'\n\
            OR lower(relativePath) LIKE '%addressbook%.db'\n\
            OR lower(relativePath) LIKE '%contacts%.sqlitedb'\n\
            OR lower(relativePath) LIKE '%contacts%.sqlite'\n\
            OR lower(relativePath) LIKE '%contacts%.db'\n\
            OR lower(relativePath) LIKE '%/abperson%'\n\
            OR lower(relativePath) LIKE '%/abstore%'\n\
         ORDER BY\n\
            CASE\n\
              WHEN lower(relativePath) LIKE '%addressbook.sqlitedb' THEN 0\n\
              WHEN lower(relativePath) LIKE '%addressbook%.sqlite%' THEN 1\n\
              WHEN lower(relativePath) LIKE '%contacts%.sqlite%' THEN 2\n\
              WHEN lower(relativePath) LIKE '%.vcf' THEN 3\n\
              ELSE 4\n\
            END,\n\
            CASE WHEN domain = 'HomeDomain' THEN 0 ELSE 1 END,\n\
            domain, relativePath",
    )?;

    statement
        .query_map([], |row| {
            Ok(ManifestPathRecord {
                file_id: row.get(0)?,
                domain: row.get(1)?,
                relative_path: row.get(2)?,
                flags: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Kontaktkandidaten konnten nicht aus Manifest.db gelesen werden")
}

pub fn parse_file_encryption_metadata(blob: &[u8]) -> Result<FileEncryptionMetadata> {
    let archive = Value::from_reader(Cursor::new(blob))
        .context("Dateimetadaten sind keine lesbare binäre Property-List")?;
    let archive_dict = archive
        .as_dictionary()
        .context("Dateimetadaten-Archiv ist kein Dictionary")?;
    let objects = archive_dict
        .get("$objects")
        .and_then(Value::as_array)
        .context("NSKeyedArchive enthält kein $objects-Array")?;

    let metadata_dict = objects
        .iter()
        .filter_map(Value::as_dictionary)
        .find(|dict| dict.contains_key("EncryptionKey") && dict.contains_key("ProtectionClass"))
        .context("EncryptionKey und ProtectionClass wurden im Dateimetadaten-Archiv nicht gefunden")?;

    let protection_class = metadata_dict
        .get("ProtectionClass")
        .and_then(value_u64)
        .and_then(|value| u32::try_from(value).ok())
        .context("ProtectionClass ist keine gültige Ganzzahl")?;

    let encryption_ref = metadata_dict
        .get("EncryptionKey")
        .context("EncryptionKey fehlt")?;
    let encryption_value = resolve_archive_value(objects, encryption_ref)?;
    let key_data = extract_data(objects, encryption_value)
        .context("EncryptionKey enthält keine NSData-Nutzdaten")?;
    if key_data.len() < 5 {
        bail!("EncryptionKey ist zu kurz: {} Bytes", key_data.len());
    }

    let class_prefix = u32::from_le_bytes(key_data[..4].try_into().expect("four-byte prefix"));
    if class_prefix != protection_class {
        bail!(
            "ProtectionClass {} stimmt nicht mit dem EncryptionKey-Präfix {} überein",
            protection_class,
            class_prefix
        );
    }

    Ok(FileEncryptionMetadata {
        protection_class,
        wrapped_key: key_data[4..].to_vec(),
        logical_size: metadata_dict.get("Size").and_then(value_u64),
        size_before_copy: metadata_dict.get("SizeBeforeCopy").and_then(value_u64),
        content_compression_method: metadata_dict
            .get("ContentCompressionMethod")
            .and_then(value_u64),
        content_encoding_method: metadata_dict
            .get("ContentEncodingMethod")
            .and_then(value_u64),
    })
}

fn resolve_archive_value<'a>(objects: &'a [Value], value: &'a Value) -> Result<&'a Value> {
    if let Value::Uid(uid) = value {
        return objects
            .get(uid.get() as usize)
            .context("NSKeyedArchive-UID liegt außerhalb des $objects-Arrays");
    }
    if let Some(dict) = value.as_dictionary() {
        if let Some(index) = dict.get("CF$UID").and_then(value_u64) {
            return objects
                .get(index as usize)
                .context("CF$UID liegt außerhalb des $objects-Arrays");
        }
    }
    Ok(value)
}

fn extract_data<'a>(objects: &'a [Value], value: &'a Value) -> Option<&'a [u8]> {
    match value {
        Value::Data(data) => Some(data.as_slice()),
        Value::Dictionary(dict) => {
            for key in ["NS.data", "data"] {
                if let Some(inner) = dict.get(key) {
                    if let Ok(resolved) = resolve_archive_value(objects, inner) {
                        if let Some(data) = extract_data(objects, resolved) {
                            return Some(data);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn value_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Integer(integer) => integer.as_unsigned(),
        _ => None,
    }
}

pub fn manifest_file_count(database_path: &Path) -> Result<u64> {
    let connection = Connection::open(database_path)
        .with_context(|| format!("Entschlüsselte Manifest.db kann nicht geöffnet werden: {}", database_path.display()))?;
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM Files", [], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_plist_metadata() {
        let error = parse_file_encryption_metadata(b"not a plist").expect_err("must reject");
        assert!(error.to_string().contains("Property-List"));
    }
}
