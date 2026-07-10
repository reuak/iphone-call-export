use anyhow::{bail, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use iphone_call_export_backup::{default_backup_root, inspect_backup, newest_backup};
use iphone_call_export_manifest::{
    decrypt_manifest_db, find_call_history_record, inspect_manifest_db, keybag_tag_u32,
    manifest_file_count, parse_file_encryption_metadata, read_encrypted_backup_metadata,
    unlock_manifest_key,
};
use std::{path::PathBuf, time::Duration};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

#[derive(Debug, Parser)]
#[command(name = "iphone-call-export")]
#[command(about = "Liest iPhone-Finder-Backups für die spätere Zeiterfassung aus")]
struct Args {
    /// Optionaler Pfad zum MobileSync/Backup-Ordner
    #[arg(long)]
    backup_root: Option<PathBuf>,

    /// Backup-Passwort lokal abfragen, Manifest.db entschlüsseln und Anrufdaten suchen
    #[arg(long)]
    unlock: bool,
}

fn spinner(message: impl Into<String>) -> Result<ProgressBar> {
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg} [{elapsed_precise}]")?
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    progress.set_message(message.into());
    progress.enable_steady_tick(Duration::from_millis(90));
    Ok(progress)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let root = args.backup_root.unwrap_or(default_backup_root()?);

    println!("Suche Backups in: {}", root.display());
    let backup = newest_backup(&root)?;
    let info = inspect_backup(&backup)?;

    println!("\n✓ Backup gefunden");
    println!("  Pfad: {}", info.path);
    println!("  Gerät: {}", info.device_name.as_deref().unwrap_or("unbekannt"));
    println!("  iOS: {}", info.product_version.as_deref().unwrap_or("unbekannt"));
    println!(
        "  Verschlüsselt: {}",
        match info.encrypted {
            Some(true) => "ja",
            Some(false) => "nein",
            None => "unbekannt",
        }
    );

    let manifest = inspect_manifest_db(&backup)?;
    println!("\n✓ Manifest.db gefunden");
    println!("  Größe: {} Bytes", manifest.size_bytes);
    println!(
        "  Format: {}",
        if manifest.is_plain_sqlite {
            "unverschlüsselte SQLite-Datenbank"
        } else {
            "verschlüsselt oder kein direkt lesbares SQLite-Format"
        }
    );

    if info.encrypted == Some(true) && !manifest.is_plain_sqlite {
        let encrypted = read_encrypted_backup_metadata(&backup)?;
        println!("\n✓ Verschlüsselungsmetadaten gelesen");
        println!(
            "  Backupformat: {}",
            encrypted.version.as_deref().unwrap_or("unbekannt")
        );
        println!("  Keybag-Größe: {} Bytes", encrypted.backup_keybag.len());
        println!("  Keybag-Einträge: {}", encrypted.keybag_entries.len());
        println!("  Manifest-Schlüsselklasse: {}", encrypted.manifest_key_class);
        println!(
            "  Eingewickelter Manifest-Schlüssel: {} Bytes",
            encrypted.wrapped_manifest_key.len()
        );
        if let Some(iter) = keybag_tag_u32(&encrypted.keybag_entries, "ITER") {
            println!("  PBKDF2-ITER: {iter}");
        }
        if let Some(dpic) = keybag_tag_u32(&encrypted.keybag_entries, "DPIC") {
            println!("  PBKDF2-DPIC: {dpic}");
        }

        if args.unlock {
            println!("\nDas Passwort wird lokal und unsichtbar eingegeben.");
            let password = Zeroizing::new(rpassword::prompt_password("Backup-Passwort: ")?);
            if password.is_empty() {
                bail!("Kein Passwort eingegeben");
            }

            let progress = spinner("Backup-Schlüssel wird abgeleitet (10.000.000 + 10.000 Runden) …")?;
            let manifest_key = unlock_manifest_key(&encrypted, password.as_bytes());
            progress.finish_and_clear();
            let Some(manifest_key) = manifest_key? else {
                bail!("Passwort falsch oder Backup-Schlüssel konnten nicht entsperrt werden");
            };

            println!("✓ Passwort korrekt");
            println!("✓ Manifest-Schlüssel entsperrt");

            let temp = NamedTempFile::new()?;
            let decrypt_progress = spinner("Manifest.db wird vollständig entschlüsselt …")?;
            let decrypted_size = decrypt_manifest_db(
                &backup.join("Manifest.db"),
                temp.path(),
                &manifest_key,
            );
            decrypt_progress.finish_and_clear();
            let decrypted_size = decrypted_size?;

            println!("✓ Manifest.db vollständig entschlüsselt ({decrypted_size} Bytes)");
            let count = manifest_file_count(temp.path())?;
            println!("✓ SQLite geöffnet: {count} Dateieinträge");

            match find_call_history_record(temp.path())? {
                Some(record) => {
                    println!("✓ CallHistory.storedata gefunden");
                    println!("  Domain: {}", record.domain);
                    println!("  Relativer Pfad: {}", record.relative_path);
                    println!("  Backup-Datei-ID: {}", record.file_id);
                    println!("  Physischer Pfad: {}", backup.join(&record.file_id[..2]).join(&record.file_id).display());
                    println!("  Metadaten-BLOB: {} Bytes", record.metadata_blob.len());

                    let file_crypto = parse_file_encryption_metadata(&record.metadata_blob)?;
                    println!("✓ Dateiverschlüsselungsmetadaten gelesen");
                    println!("  Schutzklasse: {}", file_crypto.protection_class);
                    println!("  Eingewickelter Dateischlüssel: {} Bytes", file_crypto.wrapped_key.len());
                    match file_crypto.logical_size {
                        Some(size) => println!("  Logische Dateigröße: {size} Bytes"),
                        None => println!("  Logische Dateigröße: unbekannt"),
                    }
                    println!("\nNächster Schritt: Dateischlüssel entsperren und CallHistory.storedata entschlüsseln.");
                }
                None => {
                    println!("⚠ CallHistory.storedata wurde in Manifest.db nicht gefunden");
                }
            }

            println!("\nDie entschlüsselte Manifest.db wurde nur temporär angelegt und wird jetzt gelöscht.");
        } else {
            println!("\nNächster Test:");
            println!("  cargo run -p iphone-call-export -- --unlock");
            println!("Das Passwort wird weder gespeichert noch ausgegeben.");
        }
    }

    Ok(())
}
