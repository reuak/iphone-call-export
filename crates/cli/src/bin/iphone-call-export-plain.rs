use anyhow::{bail, Context, Result};
use clap::Parser;
use iphone_call_export_backup::{default_backup_root, inspect_backup, newest_backup};
use iphone_call_export_manifest::{
    export_calls_csv_with_contacts, find_call_history_record, find_primary_addressbook_record,
    inspect_call_history_schema, load_contact_index, ContactIndex,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "iphone-call-export-plain")]
#[command(about = "Liest unverschlüsselte iPhone-Finder-Backups aus")]
struct Args {
    #[arg(long)]
    backup_root: Option<PathBuf>,

    #[arg(long)]
    unlock: bool,

    #[arg(long, value_name = "DATEI")]
    csv: Option<PathBuf>,

    #[arg(long)]
    find_contacts: bool,

    #[arg(long)]
    password_stdin: bool,
}

fn physical_path(backup: &Path, file_id: &str) -> Result<PathBuf> {
    if file_id.len() < 2 {
        bail!("Ungültige Backup-Datei-ID: {file_id}");
    }
    Ok(backup.join(&file_id[..2]).join(file_id))
}

fn export_calls(database: &Path, csv: Option<&Path>, contacts: Option<&ContactIndex>) -> Result<()> {
    let schema = inspect_call_history_schema(database)?;
    println!("✓ Anrufdatenbank als SQLite geöffnet");
    if let Some(count) = schema.call_count {
        println!("  Anrufdatensätze: {count}");
    }
    let output = csv.context("Kein CSV-Ausgabepfad angegeben")?;
    let stats = export_calls_csv_with_contacts(database, output, contacts)?;
    println!("✓ {} Anrufe als CSV exportiert", stats.exported);
    if contacts.is_some() {
        println!("✓ {} Anrufzeilen mit AddressBook-Kontakten abgeglichen", stats.matched_contacts);
    }
    println!("  Ausgabe: {}", output.display());
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let root = args.backup_root.clone().unwrap_or(default_backup_root()?);
    println!("Suche Backups in: {}", root.display());
    let backup = newest_backup(&root)?;
    let info = inspect_backup(&backup)?;

    println!("\n✓ Backup gefunden");
    println!("  Pfad: {}", info.path);
    println!("  Gerät: {}", info.device_name.as_deref().unwrap_or("unbekannt"));
    println!("  iOS: {}", info.product_version.as_deref().unwrap_or("unbekannt"));
    println!("  Verschlüsselt: {}", if info.encrypted == Some(false) { "nein" } else { "unbekannt" });

    if info.encrypted == Some(true) {
        bail!("Dieses Hilfsprogramm ist nur für unverschlüsselte Backups vorgesehen");
    }

    let manifest_path = backup.join("Manifest.db");
    if !manifest_path.is_file() {
        bail!("Manifest.db fehlt: {}", manifest_path.display());
    }

    let contacts = if args.find_contacts {
        match find_primary_addressbook_record(&manifest_path)? {
            Some(record) => {
                let path = physical_path(&backup, &record.file_id)?;
                if path.is_file() {
                    let index = load_contact_index(&path).with_context(|| {
                        format!("AddressBook-Datenbank kann nicht gelesen werden: {}", path.display())
                    })?;
                    println!("✓ {} Telefonnummern aus AddressBook für den Abgleich geladen", index.phone_count);
                    Some(index)
                } else {
                    println!("⚠ AddressBook-Datei fehlt: {}", path.display());
                    None
                }
            }
            None => {
                println!("⚠ Primäre AddressBook.sqlitedb wurde nicht gefunden");
                None
            }
        }
    } else {
        None
    };

    let record = find_call_history_record(&manifest_path)?
        .context("CallHistory.storedata wurde in Manifest.db nicht gefunden")?;
    let call_path = physical_path(&backup, &record.file_id)?;
    if !call_path.is_file() {
        bail!("CallHistory-Datei fehlt: {}", call_path.display());
    }

    export_calls(&call_path, args.csv.as_deref(), contacts.as_ref())
}
