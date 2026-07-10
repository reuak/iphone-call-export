# iphone-call-export

Rust-Werkzeug zum lokalen Auslesen eines iPhone-Finder-Backups und zum späteren Export von Anrufdaten für die Zeiterfassung.

## Aktueller Stand

Die erste Version:

- findet den macOS-Backup-Ordner automatisch,
- wählt das zuletzt geänderte iPhone-Backup,
- liest `Info.plist` und `Manifest.plist`,
- zeigt Gerätename, iOS-Version und Verschlüsselungsstatus an,
- gibt bei fehlendem macOS-Zugriff einen verständlichen Fehler aus.

Noch nicht implementiert:

- Entschlüsselung von `Manifest.db`,
- Extraktion von `CallHistory.storedata`,
- Nextcloud-vCard-Zuordnung,
- Zeitraumfilter,
- Excel-Ausgabe,
- grafische macOS-App.

## Voraussetzungen

Rust installieren:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Auf macOS muss Terminal unter **Systemeinstellungen → Datenschutz & Sicherheit → Vollständiger Festplattenzugriff** freigegeben sein.

## Testen

```bash
git clone https://github.com/reuak/iphone-call-export.git
cd iphone-call-export
cargo run -p iphone-call-export
```

Alternativer Backup-Pfad:

```bash
cargo run -p iphone-call-export -- --backup-root "/Users/%user%/Library/Application Support/MobileSync/Backup"
```

## Datenschutz

Backup-Passwort, Kontakte und Anrufdaten sollen ausschließlich lokal verarbeitet werden. Solche Dateien dürfen nicht in das Repository eingecheckt werden.
