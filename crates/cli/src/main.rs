use anyhow::{bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use iphone_call_export_backup::{default_backup_root, inspect_backup, newest_backup};
use iphone_call_export_manifest::{
    decrypt_backup_file, decrypt_backup_payload, decrypt_manifest_db, find_call_history_record,
    inspect_call_history_schema, inspect_manifest_db, keybag_tag_u32, manifest_file_count,
    parse_file_encryption_metadata, read_encrypted_backup_metadata, unlock_backup,
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

    /// Backup-Passwort lokal abfragen, Manifest.db und Anrufdaten entschlüsseln
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

fn show_optional_number(label: &str, value: Option<u64>) {
    match value {
        Some(value) => println!("  {label}: {value}"),
        None => println!("  {label}: nicht gesetzt"),
    }
}

fn printable_header(header: &[u8; 16]) -> String {
    header
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            }
        })
        .collect()
}

fn hex_header(header: &[u8; 16]) -> String {
    header
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn show_call_history_schema(path: &std::path::Path) -> Result<()> {
    let schema = inspect_call_history_schema(path)?;
    println!("✓ Entschlüsselte Anrufdatenbank als SQLite geöffnet");
    println!("  Tabellen: {}", schema.tables.join(", "));
    match schema.call_table {
        Some(table) => {
            println!("✓ Anruftabelle erkannt: {table}");
            if let Some(count) = schema.call_count {
                println!("  Anrufdatensätze: {count}");
            }
            println!("  Spalten: {}", schema.call_columns.join(", "));
            println!("\nNächster Schritt: relevante Spalten zuordnen und Anrufe nach Zeitraum exportieren.");
        }
        None => {
            println!("⚠ Keine eindeutige Anruftabelle erkannt");
            println!("Bitte den Tabellenabschnitt posten; danach wird die Erkennung angepasst.");
        }
    }
    Ok(())
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

            let progress = spinner("Backup-Schlüssel wird einmalig abgeleitet (10.000.000 + 10.000 Runden) …")?;
            let unlocked = unlock_backup(&encrypted, password.as_bytes());
            progress.finish_and_clear();
            let Some(unlocked) = unlocked? else {
                bail!("Passwort falsch oder Backup-Schlüssel konnten nicht entsperrt werden");
            };

            println!("✓ Passwort korrekt");
            println!("✓ Manifest- und Klassenschlüssel entsperrt");

            let manifest_temp = NamedTempFile::new()?;
            let decrypt_progress = spinner("Manifest.db wird vollständig entschlüsselt …")?;
            let decrypted_size = decrypt_manifest_db(
                &backup.join("Manifest.db"),
                manifest_temp.path(),
                unlocked.manifest_key(),
            );
            decrypt_progress.finish_and_clear();
            let decrypted_size = decrypted_size?;

            println!("✓ Manifest.db vollständig entschlüsselt ({decrypted_size} Bytes)");
            let count = manifest_file_count(manifest_temp.path())?;
            println!("✓ SQLite geöffnet: {count} Dateieinträge");

            match find_call_history_record(manifest_temp.path())? {
                Some(record) => {
                    println!("✓ CallHistory.storedata gefunden");
                    println!("  Domain: {}", record.domain);
                    println!("  Relativer Pfad: {}", record.relative_path);
                    println!("  Backup-Datei-ID: {}", record.file_id);
                    let physical_path = backup.join(&record.file_id[..2]).join(&record.file_id);
                    println!("  Physischer Pfad: {}", physical_path.display());
                    println!("  Physische Dateigröße: {} Bytes", physical_path.metadata()?.len());
                    println!("  Metadaten-BLOB: {} Bytes", record.metadata_blob.len());

                    let file_crypto = parse_file_encryption_metadata(&record.metadata_blob)?;
                    println!("✓ Datei- und Kompressionsmetadaten gelesen");
                    println!("  Schutzklasse: {}", file_crypto.protection_class);
                    println!(
                        "  Eingewickelter Dateischlüssel: {} Bytes",
                        file_crypto.wrapped_key.len()
                    );
                    match file_crypto.logical_size {
                        Some(size) => println!("  Logische Dateigröße: {size} Bytes"),
                        None => println!("  Logische Dateigröße: nicht gesetzt"),
                    }
                    match file_crypto.size_before_copy {
                        Some(size) => println!("  SizeBeforeCopy: {size} Bytes"),
                        None => println!("  SizeBeforeCopy: nicht gesetzt"),
                    }
                    show_optional_number(
                        "ContentCompressionMethod",
                        file_crypto.content_compression_method,
                    );
                    show_optional_number(
                        "ContentEncodingMethod",
                        file_crypto.content_encoding_method,
                    );

                    let file_key = unlocked.unlock_file_key(&file_crypto)?;
                    println!("✓ Dateischlüssel entsperrt");

                    let physical_size = physical_path.metadata()?.len();
                    let logical_size = file_crypto
                        .logical_size
                        .context("Logische Dateigröße fehlt; sichere Entschlüsselung ist nicht möglich")?;

                    if logical_size > physical_size
                        || file_crypto.content_compression_method.unwrap_or(0) != 0
                        || file_crypto.content_encoding_method.unwrap_or(0) != 0
                    {
                        let payload_temp = NamedTempFile::new()?;
                        let payload_progress = spinner("Gespeicherter CallHistory-Inhalt wird vollständig entschlüsselt …")?;
                        let payload = decrypt_backup_payload(
                            &physical_path,
                            payload_temp.path(),
                            &file_key,
                        );
                        payload_progress.finish_and_clear();
                        let payload = payload?;

                        println!("\n✓ Gespeicherter Inhalt entschlüsselt ({} Bytes)", payload.size_bytes);
                        println!("  Kopf als Text: {}", printable_header(&payload.header));
                        println!("  Kopf als Hex: {}", hex_header(&payload.header));
                        println!("  Direktes SQLite: {}", if payload.is_sqlite { "ja" } else { "nein" });
                        if payload.is_sqlite {
                            show_call_history_schema(payload_temp.path())?;
                        } else {
                            println!("Nächster Schritt: Format anhand dieses Dateikopfs erkennen und dekodieren.");
                        }
                    } else {
                        let call_history_temp = NamedTempFile::new()?;
                        let file_progress = spinner("CallHistory.storedata wird entschlüsselt …")?;
                        let call_history_size = decrypt_backup_file(
                            &physical_path,
                            call_history_temp.path(),
                            &file_key,
                            logical_size,
                        );
                        file_progress.finish_and_clear();
                        let call_history_size = call_history_size?;

                        println!(
                            "✓ CallHistory.storedata vollständig entschlüsselt ({call_history_size} Bytes)"
                        );
                        show_call_history_schema(call_history_temp.path())?;
                    }

                    println!(
                        "\nDie entschlüsselten Datenbanken wurden nur temporär angelegt und werden jetzt gelöscht."
                    );
                }
                None => {
                    println!("⚠ CallHistory.storedata wurde in Manifest.db nicht gefunden");
                }
            }
        } else {
            println!("\nNächster Test:");
            println!("  cargo run -p iphone-call-export -- --unlock");
            println!("Das Passwort wird weder gespeichert noch ausgegeben.");
        }
    }

    Ok(())
}
