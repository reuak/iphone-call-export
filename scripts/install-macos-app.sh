#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="iPhone Call Export"
INSTALL_DIR="${HOME}/Applications"
APP_DIR="${INSTALL_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

cd "$ROOT_DIR"
echo "Baue optimierte CLI …"
cargo build --release -p iphone-call-export

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp target/release/iphone-call-export "$RESOURCES_DIR/iphone-call-export"
chmod 700 "$RESOURCES_DIR/iphone-call-export"

cat > "$CONTENTS_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>iPhone Call Export</string>
  <key>CFBundleExecutable</key><string>iphone-call-export-gui</string>
  <key>CFBundleIdentifier</key><string>de.reuak.iphone-call-export</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>iPhone Call Export</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.1</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

cat > "$MACOS_DIR/iphone-call-export-gui" <<'LAUNCHER'
#!/bin/zsh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CLI="$HERE/../Resources/iphone-call-export"

osascript - "$CLI" <<'APPLESCRIPT'
on run argv
  set cliPath to item 1 of argv
  set homePath to POSIX path of (path to home folder)
  set defaultBackupRoot to homePath & "Library/Application Support/MobileSync/Backup"

  set backupMode to button returned of (display dialog ¬
    "Die App sucht Finder-iPhone-Backups automatisch am üblichen macOS-Speicherort. Nur bei Backups auf einem anderen Datenträger muss ein Ordner gewählt werden." ¬
    buttons {"Anderen Ordner wählen", "Automatisch suchen"} ¬
    default button "Automatisch suchen" ¬
    with title "iPhone Call Export")

  set backupArgument to ""
  if backupMode is "Anderen Ordner wählen" then
    set backupChoice to choose folder with prompt "MobileSync/Backup-Ordner oder konkreten Geräte-Backup-Ordner auswählen" default location (path to home folder)
    set backupPath to POSIX path of backupChoice
    set backupArgument to " --backup-root " & quoted form of backupPath
  else
    set backupArgument to " --backup-root " & quoted form of defaultBackupRoot
  end if

  set outputChoice to choose file name ¬
    with prompt "CSV-Datei speichern" ¬
    default location (path to home folder) ¬
    default name "iphone-anrufe-mit-kontakten.csv"
  set outputPath to POSIX path of outputChoice

  set contactChoice to button returned of (display dialog ¬
    "Kontakte aus dem iPhone-AddressBook abgleichen?" ¬
    buttons {"Ohne Kontakte", "Mit Kontakten"} ¬
    default button "Mit Kontakten" ¬
    with title "iPhone Call Export")

  set cmd to quoted form of cliPath & " --unlock" & backupArgument & " --csv " & quoted form of outputPath
  if contactChoice is "Mit Kontakten" then set cmd to cmd & " --find-contacts"

  tell application "Terminal"
    activate
    do script cmd
  end tell
end run
APPLESCRIPT
LAUNCHER
chmod 700 "$MACOS_DIR/iphone-call-export-gui"

mkdir -p "$INSTALL_DIR"
touch "$APP_DIR"

echo
echo "✓ Installiert: $APP_DIR"
echo "Öffnen mit:"
echo "  open \"$APP_DIR\""
echo
echo "Die App sucht Finder-Backups standardmäßig automatisch unter:"
echo "  $HOME/Library/Application Support/MobileSync/Backup"
echo "Der CSV-Dialog startet im Benutzerordner:"
echo "  $HOME"
echo
echo "Hinweis: Terminal benötigt vollständigen Festplattenzugriff für MobileSync-Backups."
