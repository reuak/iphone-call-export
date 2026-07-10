use anyhow::{Context, Result};
use directories::UserDirs;
use iphone_call_export_common::BackupInfo;
use plist::Value;
use std::{fs, path::{Path, PathBuf}, time::SystemTime};

pub fn default_backup_root() -> Result<PathBuf> {
    let home = UserDirs::new()
        .context("Benutzerverzeichnis konnte nicht ermittelt werden")?
        .home_dir()
        .to_path_buf();

    Ok(home.join("Library/Application Support/MobileSync/Backup"))
}

pub fn newest_backup(root: &Path) -> Result<PathBuf> {
    let entries = fs::read_dir(root).with_context(|| {
        format!(
            "Backup-Ordner kann nicht gelesen werden: {}. Auf macOS benötigt Terminal bzw. die App möglicherweise vollständigen Festplattenzugriff.",
            root.display()
        )
    })?;

    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || !path.join("Manifest.plist").is_file() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((modified, path));
    }

    candidates
        .into_iter()
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
        .context("Kein iPhone-Backup mit Manifest.plist gefunden")
}

pub fn inspect_backup(path: &Path) -> Result<BackupInfo> {
    let info = read_dictionary(&path.join("Info.plist"))?;
    let manifest = read_dictionary(&path.join("Manifest.plist"))?;

    Ok(BackupInfo {
        path: path.display().to_string(),
        device_name: string_value(&info, "Device Name"),
        product_version: string_value(&info, "Product Version"),
        encrypted: bool_value(&manifest, "IsEncrypted"),
    })
}

fn read_dictionary(path: &Path) -> Result<plist::Dictionary> {
    let value = Value::from_file(path)
        .with_context(|| format!("Property-List kann nicht gelesen werden: {}", path.display()))?;
    value
        .into_dictionary()
        .with_context(|| format!("Property-List ist kein Dictionary: {}", path.display()))
}

fn string_value(dict: &plist::Dictionary, key: &str) -> Option<String> {
    dict.get(key)?.as_string().map(ToOwned::to_owned)
}

fn bool_value(dict: &plist::Dictionary, key: &str) -> Option<bool> {
    dict.get(key)?.as_boolean()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_ends_with_mobile_sync_backup() {
        let path = default_backup_root().expect("home directory");
        assert!(path.ends_with("Library/Application Support/MobileSync/Backup"));
    }
}
