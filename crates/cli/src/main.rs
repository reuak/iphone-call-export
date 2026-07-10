use anyhow::{bail, Result};
use clap::Parser;
use iphone_call_export_backup::{default_backup_root, inspect_backup, newest_backup};
use iphone_call_export_manifest::{
    inspect_manifest_db, keybag_tag_u32, read_encrypted_backup_metadata,
    verify_backup_password,
};
use std::path::PathBuf;
use zeroize::Zeroizing;

#[derive(Debug, Parser)]
#[command(name = "iphone-call-export")]
#[command(about = "Liest iPhone-Finder-Backups für die spätere Zeiterfassung aus")]
struct Args {
    /// Optionaler Pfad zum MobileSync/Backup-Ordner
    #[arg(long)]
    backup_root: Option<PathBuf>,

    /// Backup-Passwort lokal abfragen und Manifest.db testweise entsperren
    #[arg(long)]
    unlock: bool,
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
            println!("Die Ableitung mit DPIC kann einige Sekunden dauern.");
            let password = Zeroizing::new(rpassword::prompt_password("Backup-Passwort: ")?);
            if password.is_empty() {
                bail!("Kein Passwort eingegeben");
            }

            if verify_backup_password(&backup, &encrypted, password.as_bytes())? {
                println!("\n✓ Passwort korrekt");
                println!("✓ Manifest-Schlüssel entsperrt");
                println!("✓ Entschlüsselter Manifest.db-Kopf ist gültiges SQLite");
            } else {
                bail!("Passwort falsch oder Backup-Schlüssel konnten nicht entsperrt werden");
            }
        } else {
            println!("\nNächster Test:");
            println!("  cargo run -p iphone-call-export -- --unlock");
            println!("Das Passwort wird weder gespeichert noch ausgegeben.");
        }
    }

    Ok(())
}
