use anyhow::Result;
use clap::Parser;
use iphone_call_export_backup::{default_backup_root, inspect_backup, newest_backup};
use iphone_call_export_manifest::inspect_manifest_db;
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
        println!("\nNächster Schritt: Manifest.db lokal mit dem Backup-Passwort entsperren.");
        println!("Das Passwort wird dabei weder gespeichert noch ausgegeben.");
    }

    Ok(())
}
