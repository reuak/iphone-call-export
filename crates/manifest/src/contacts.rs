use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBookSchema {
    pub tables: Vec<String>,
    pub person_table: Option<String>,
    pub person_columns: Vec<String>,
    pub multivalue_table: Option<String>,
    pub multivalue_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactMatch {
    pub name: String,
    pub organization: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default)]
pub struct ContactIndex {
    exact: HashMap<String, ContactMatch>,
    suffix: HashMap<String, Option<ContactMatch>>,
    pub phone_count: u64,
}

impl ContactIndex {
    pub fn find(&self, phone: &str) -> Option<&ContactMatch> {
        let keys = phone_keys(phone);
        for key in &keys {
            if let Some(contact) = self.exact.get(key) {
                return Some(contact);
            }
        }
        let digits = digits_only(phone);
        if digits.len() >= 9 {
            let suffix = &digits[digits.len() - 9..];
            if let Some(Some(contact)) = self.suffix.get(suffix) {
                return Some(contact);
            }
        }
        None
    }
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

/// Reads phone numbers from the classic iOS AddressBook schema.
/// ABMultiValue.property = 3 is the phone property. A conservative fallback
/// accepts phone-like values when an iOS variant stores a different code.
pub fn load_contact_index(database_path: &Path) -> Result<ContactIndex> {
    let connection = Connection::open(database_path).with_context(|| {
        format!(
            "Entschlüsselte AddressBook-Datenbank kann nicht geöffnet werden: {}",
            database_path.display()
        )
    })?;

    let query = "SELECT m.value, m.property, \
        COALESCE(NULLIF(TRIM(p.DisplayName), ''), \
                 NULLIF(TRIM(COALESCE(p.First, '') || ' ' || COALESCE(p.Last, '')), ''), \
                 NULLIF(TRIM(p.Organization), ''), ''), \
        COALESCE(p.Organization, '') \
        FROM ABMultiValue m \
        JOIN ABPerson p ON p.ROWID = m.record_id \
        WHERE m.value IS NOT NULL";

    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut index = ContactIndex::default();
    for row in rows {
        let (phone, property, name, organization) = row?;
        let digits = digits_only(&phone);
        let looks_like_phone = digits.len() >= 5
            && phone
                .chars()
                .all(|c| c.is_ascii_digit() || "+-()/ .#*".contains(c));
        if property != Some(3) && !looks_like_phone {
            continue;
        }

        let contact = ContactMatch {
            name: if name.is_empty() { phone.clone() } else { name },
            organization,
            phone: phone.clone(),
        };
        let keys = phone_keys(&phone);
        if keys.is_empty() {
            continue;
        }
        for key in keys {
            index.exact.entry(key).or_insert_with(|| contact.clone());
        }
        if digits.len() >= 9 {
            let suffix = digits[digits.len() - 9..].to_owned();
            match index.suffix.entry(suffix) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(contact.clone()));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if entry.get().as_ref() != Some(&contact) {
                        entry.insert(None);
                    }
                }
            }
        }
        index.phone_count += 1;
    }

    Ok(index)
}

fn digits_only(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

fn phone_keys(value: &str) -> Vec<String> {
    let mut digits = digits_only(value);
    if digits.is_empty() {
        return Vec::new();
    }
    if digits.starts_with("00") {
        digits.drain(..2);
    }

    let mut keys = vec![digits.clone()];
    if let Some(rest) = digits.strip_prefix("49") {
        if !rest.is_empty() {
            keys.push(format!("0{rest}"));
        }
    } else if let Some(rest) = digits.strip_prefix('0') {
        if !rest.is_empty() {
            keys.push(format!("49{rest}"));
        }
    }
    keys.sort();
    keys.dedup();
    keys
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
            tables
                .iter()
                .find(|table| {
                    let lower = table.to_ascii_lowercase();
                    lower.contains("person") || lower.contains("contact")
                })
                .cloned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_german_local_and_international_numbers() {
        let mut index = ContactIndex::default();
        index.exact.insert(
            "491701234567".to_owned(),
            ContactMatch {
                name: "Test".to_owned(),
                organization: String::new(),
                phone: "+49 170 1234567".to_owned(),
            },
        );
        assert_eq!(index.find("0170 1234567").map(|c| c.name.as_str()), Some("Test"));
    }
}