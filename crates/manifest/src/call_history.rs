use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHistorySchema {
    pub tables: Vec<String>,
    pub call_table: Option<String>,
    pub call_columns: Vec<String>,
    pub call_count: Option<u64>,
}

pub fn inspect_call_history_schema(database_path: &Path) -> Result<CallHistorySchema> {
    let connection = Connection::open(database_path).with_context(|| {
        format!(
            "Entschlüsselte CallHistory-Datenbank kann nicht geöffnet werden: {}",
            database_path.display()
        )
    })?;

    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let call_table = tables
        .iter()
        .find(|name| name.eq_ignore_ascii_case("ZCALLRECORD"))
        .or_else(|| {
            tables.iter().find(|name| {
                let lower = name.to_ascii_lowercase();
                lower.contains("call") && lower.contains("record")
            })
        })
        .cloned();

    let (call_columns, call_count) = if let Some(table) = &call_table {
        let quoted = table.replace('"', "\"\"");
        let mut columns_statement =
            connection.prepare(&format!("PRAGMA table_info(\"{quoted}\")"))?;
        let columns = columns_statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let count: i64 = connection.query_row(
            &format!("SELECT COUNT(*) FROM \"{quoted}\""),
            [],
            |row| row.get(0),
        )?;
        (columns, Some(count.max(0) as u64))
    } else {
        (Vec::new(), None)
    };

    Ok(CallHistorySchema {
        tables,
        call_table,
        call_columns,
        call_count,
    })
}
