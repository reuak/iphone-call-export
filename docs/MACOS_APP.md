# macOS-App

## Installation

Die erste App-Version baut die vorhandene Rust-CLI optimiert und installiert einen lokalen macOS-App-Launcher unter `~/Applications`.

```bash
cd ~/iphone-call-export
git switch feature/macos-app
git pull
chmod +x scripts/install-macos-app.sh
./scripts/install-macos-app.sh
open "$HOME/Applications/iPhone Call Export.app"
```

Die Oberfläche fragt nacheinander nach:

1. dem MobileSync-Backup-Ordner,
2. dem Ziel für die CSV-Datei,
3. dem Kontaktabgleich.

Danach öffnet sie Terminal und startet den Export. Das Backup-Passwort wird weiterhin lokal und verdeckt im Terminal eingegeben; es wird nicht gespeichert oder als Kommandozeilenargument übergeben.

## macOS-Berechtigung

Terminal benötigt unter **Systemeinstellungen → Datenschutz & Sicherheit → Vollständiger Festplattenzugriff** Zugriff auf den MobileSync-Ordner.

## Aktueller Funktionsumfang

- installierbare `.app` unter `~/Applications`
- Auswahl von Backup-Ordner und CSV-Ziel
- Kontaktabgleich ein-/ausschalten
- bestehende sichere Passwortabfrage
- CSV-Export mit iPhone-AddressBook-Zuordnung

## Nächste Ausbaustufe

Die vollständige native Oberfläche wird den Terminal-Schritt ersetzen und zusätzlich Zeitraumfilter, Vorschau, VCF-Import, unbekannte Nummern und Excel-kompatible Exporte anbieten. Der Fortschritt wird in Issue #8 verfolgt.
