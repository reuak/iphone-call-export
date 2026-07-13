use crate::ContactIndex;
use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use rusqlite::Connection;
use std::path::Path;

const APPLE_EPOCH_UNIX_SECONDS: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHistorySchema {
    pub tables: Vec<String>,
    pub call_table: Option<String>,
    pub call_columns: Vec<String>,
    pub call_count: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallExportStats {
    pub exported: u64,
    pub matched_contacts: u64,
}

pub fn inspect_call_history_schema(database_path: &Path) -> Result<CallHistorySchema> {
    let connection = Connection::open(database_path).with_context(|| {
        format!("Entschlüsselte CallHistory-Datenbank kann nicht geöffnet werden: {}", database_path.display())
    })?;
    let mut statement = connection.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
    let tables = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let call_table = tables.iter().find(|name| name.eq_ignore_ascii_case("ZCALLRECORD")).or_else(|| {
        tables.iter().find(|name| {
            let lower = name.to_ascii_lowercase();
            lower.contains("call") && lower.contains("record")
        })
    }).cloned();
    let (call_columns, call_count) = if let Some(table) = &call_table {
        let quoted = table.replace('"', "\"\"");
        let mut columns_statement = connection.prepare(&format!("PRAGMA table_info(\"{quoted}\")"))?;
        let columns = columns_statement.query_map([], |row| row.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let count: i64 = connection.query_row(&format!("SELECT COUNT(*) FROM \"{quoted}\""), [], |row| row.get(0))?;
        (columns, Some(count.max(0) as u64))
    } else {
        (Vec::new(), None)
    };
    Ok(CallHistorySchema { tables, call_table, call_columns, call_count })
}

pub fn export_calls_csv(database_path: &Path, output_path: &Path) -> Result<u64> {
    Ok(export_calls_csv_with_contacts(database_path, output_path, None)?.exported)
}

pub fn export_calls_csv_with_contacts(
    database_path: &Path,
    output_path: &Path,
    contacts: Option<&ContactIndex>,
) -> Result<CallExportStats> {
    let connection = Connection::open(database_path).with_context(|| {
        format!("Entschlüsselte CallHistory-Datenbank kann nicht geöffnet werden: {}", database_path.display())
    })?;
    let mut statement = connection.prepare(
        "SELECT ZDATE, ZDURATION, ZORIGINATED, ZANSWERED, ZADDRESS, ZNAME, ZCALLTYPE, ZISO_COUNTRY_CODE, ZSERVICE_PROVIDER, ZUNIQUE_ID FROM ZCALLRECORD ORDER BY ZDATE ASC",
    )?;
    let mut writer = csv::WriterBuilder::new().delimiter(b';').from_path(output_path)
        .with_context(|| format!("CSV-Datei kann nicht erstellt werden: {}", output_path.display()))?;
    writer.write_record([
        "Datum", "Dauer_Sekunden", "Richtung", "Angenommen", "Rufnummer", "Name",
        "Name_Anrufliste", "Kontakt_Organisation", "Kontaktquelle", "Anruftyp", "Land",
        "Dienstanbieter", "Eindeutige_ID",
    ])?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<f64>>(0)?, row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<i64>>(2)?, row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<i64>>(6)?, row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?, row.get::<_, Option<String>>(9)?,
        ))
    })?;
    let mut exported = 0_u64;
    let mut matched_contacts = 0_u64;
    for row in rows {
        let (date, duration, originated, answered, address, call_name, call_type, country, provider, unique_id) = row?;
        let date_text = date.and_then(format_apple_timestamp).unwrap_or_default();
        let direction = match originated { Some(1) => "ausgehend", Some(_) => "eingehend", None => "unbekannt" };
        let answered_text = match answered { Some(1) => "ja", Some(_) => "nein", None => "unbekannt" };
        let address = address.unwrap_or_default();
        let call_name = call_name.unwrap_or_default();
        let contact = contacts.and_then(|index| index.find(&address));
        if contact.is_some() { matched_contacts += 1; }
        let resolved_name = contact.map(|item| item.name.as_str()).filter(|name| !name.is_empty()).unwrap_or(&call_name);
        let organization = contact.map(|item| item.organization.as_str()).unwrap_or("");
        let source = if contact.is_some() { "iPhone-AddressBook" } else if call_name.is_empty() { "" } else { "Anrufliste" };
        writer.write_record([
            date_text,
            duration.map(|value| value.round().to_string()).unwrap_or_default(),
            direction.to_owned(), answered_text.to_owned(), address,
            resolved_name.to_owned(), call_name, organization.to_owned(), source.to_owned(),
            call_type.map(|value| value.to_string()).unwrap_or_default(),
            country.unwrap_or_default(), provider.unwrap_or_default(), unique_id.unwrap_or_default(),
        ])?;
        exported += 1;
    }
    writer.flush()?;
    Ok(CallExportStats { exported, matched_contacts })
}

fn format_apple_timestamp(value: f64) -> Option<String> {
    if !value.is_finite() { return None; }
    let unix_seconds = value + APPLE_EPOCH_UNIX_SECONDS;
    let seconds = unix_seconds.floor() as i64;
    let nanos = ((unix_seconds - seconds as f64) * 1_000_000_000.0).round().clamp(0.0, 999_999_999.0) as u32;
    let utc: DateTime<Utc> = DateTime::from_timestamp(seconds, nanos)?;
    Some(utc.with_timezone(&Local).to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_apple_epoch() {
        let text = format_apple_timestamp(0.0).expect("timestamp");
        assert!(text.starts_with("2001-01-01T"));
    }
}
