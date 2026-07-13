use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBookSchema {
    pub tables: Vec<String>,
    pub person_table: Option<String>,
    pub person_columns: Vec<String>,
    pub multivalue_table: Option<String>,
    pub multivalue_columns: Vec<String>,
}

pub fn inspect_addressbook_schema(database_path: &Path) -> Result<AddressBookSchema> {
    let connection = Connection::open(database_path).with_context(|| {
        format!(
            "Entschlüsselte AddressBook-Datenbank kann nicht geöffnet werden: {}",
            database_path.display()
        )
    })?;

    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let person_table = find_table(&tables, &["ABPerson", "ZABCDRECORD", "ZCONTACT"]);
    let multivalue_table = find_table(&tables, &["ABMultiValue", "ZABCDPHONENUMBER", "ZPHONE"]);

    let person_columns = table_columns(&connection, person_table.as_deref())?;
    let multivalue_columns = table_columns(&connection, multivalue_table.as_deref())?;

    Ok(AddressBookSchema {
        tables,
        person_table,
        person_columns,
        multivalue_table,
        multivalue_columns,
    })
}

fn find_table(tables: &[String], preferred: &[&str]) -> Option<String> {
    preferred
        .iter()
        .find_map(|wanted| {
            tables
                .iter()
                .find(|table| table.eq_ignore_ascii_case(wanted))
                .cloned()
        })
        .or_else(|| {
            tables.iter().find(|table| {
                let lower = table.to_ascii_lowercase();
                lower.contains("person") || lower.contains("contact")
            }).cloned()
        })
}

fn table_columns(connection: &Connection, table: Option<&str>) -> Result<Vec<String>> {
    let Some(table) = table else {
        return Ok(Vec::new());
    };
    let quoted = table.replace('"', "\"\"");
    let mut statement = connection.prepare(&format!("PRAGMA table_info(\"{quoted}\")"))?;
    statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Spalten der AddressBook-Tabelle konnten nicht gelesen werden")
}
