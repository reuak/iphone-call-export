use anyhow::Result;
use clap::Parser;
use iphone_call_export_backup::{default_backup_root, inspect_backup, newest_backup};
use iphone_call_export_manifest::{
    inspect_manifest_db, keybag_tag_u32, read_encrypted_backup_metadata,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "iphone-call-export")]
#[command(about = "Liest iPhone-Finder-Backups für die spätere Zeiterfassung aus")]
struct Args {
    /// Optionaler Pfad zum MobileSync/Backup-Ordner
    #[arg(long)]
    backup_root: Option<PathBuf>,
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
        let metadata = read_encrypted_backup_metadata(&backup)?;
        println!("\n✓ Verschlüsselungsmetadaten gelesen");
        println!(
            "  Backupformat: {}",
            metadata.version.as_deref().unwrap_or("unbekannt")
        );
        println!("  Keybag-Größe: {} Bytes", metadata.backup_keybag.len());
        println!("  Keybag-Einträge: {}", metadata.keybag_entries.len());
        println!("  Manifest-Schlüsselklasse: {}", metadata.manifest_key_class);
        println!(
            "  Eingewickelter Manifest-Schlüssel: {} Bytes",
            metadata.wrapped_manifest_key.len()
        );

        if let Some(iterations) = keybag_tag_u32(&metadata.keybag_entries, "ITER") {
            println!("  PBKDF2-ITER: {iterations}");
        }
        if let Some(iterations) = keybag_tag_u32(&metadata.keybag_entries, "DPIC") {
            println!("  PBKDF2-DPIC: {iterations}");
        }

        println!("\nNächster Schritt: Passwortableitung und Keybag-Entsperrung implementieren.");
        println!("Es wurden keine Schlüssel oder Passwörter ausgegeben.");
    }

    Ok(())
}
