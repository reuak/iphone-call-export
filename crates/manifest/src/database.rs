use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFileRecord {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
}

pub fn find_call_history_record(database_path: &Path) -> Result<Option<ManifestFileRecord>> {
    let connection = Connection::open(database_path)
        .with_context(|| format!("Entschlüsselte Manifest.db kann nicht geöffnet werden: {}", database_path.display()))?;

    let mut statement = connection.prepare(
        "SELECT fileID, domain, relativePath\n\
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
            })
        })
        .optional()
        .context("CallHistory.storedata konnte in Manifest.db nicht gesucht werden")
}

pub fn manifest_file_count(database_path: &Path) -> Result<u64> {
    let connection = Connection::open(database_path)
        .with_context(|| format!("Entschlüsselte Manifest.db kann nicht geöffnet werden: {}", database_path.display()))?;
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM Files", [], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}
