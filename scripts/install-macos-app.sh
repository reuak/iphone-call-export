#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="iPhone Call Export"
APP_VERSION="0.1.3"
INSTALL_DIR="${HOME}/Applications"
APP_DIR="${INSTALL_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

cd "$ROOT_DIR"
echo "Baue optimierte CLI …"
rm -f target/release/iphone-call-export
cargo build --release -p iphone-call-export

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp target/release/iphone-call-export "$RESOURCES_DIR/iphone-call-export"
chmod 700 "$RESOURCES_DIR/iphone-call-export"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
  <key>CFBundleExecutable</key><string>iphone-call-export-gui</string>
  <key>CFBundleIdentifier</key><string>de.reuak.iphone-call-export</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${APP_VERSION}</string>
  <key>CFBundleVersion</key><string>3</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

cat > "$RESOURCES_DIR/launcher.applescript" <<'APPLESCRIPT'
use scripting additions

on run argv
  try
    set cliPath to item 1 of argv
    set homeFolder to path to home folder
    set homePath to POSIX path of homeFolder
    set defaultBackupRoot to homePath & "Library/Application Support/MobileSync/Backup"

    set backupDialog to display dialog ¬
      "Die App sucht lokale Finder-iPhone-Backups automatisch am üblichen macOS-Speicherort." ¬
      buttons {"Abbrechen", "Anderen Ordner wählen", "Automatisch suchen"} ¬
      default button "Automatisch suchen" ¬
      cancel button "Abbrechen" ¬
      with title "iPhone Call Export 0.1.3"
    set backupMode to button returned of backupDialog

    if backupMode is "Anderen Ordner wählen" then
      set backupChoice to choose folder ¬
        with prompt "MobileSync/Backup-Ordner oder konkreten Geräte-Backup-Ordner auswählen" ¬
        default location homeFolder
      set backupPath to POSIX path of backupChoice
    else
      set backupPath to defaultBackupRoot
    end if

    set outputChoice to choose file name ¬
      with prompt "CSV-Datei speichern" ¬
      default location homeFolder ¬
      default name "iphone-anrufe-mit-kontakten.csv"
    set outputPath to POSIX path of outputChoice

    set contactDialog to display dialog ¬
      "Kontakte aus dem iPhone-AddressBook abgleichen?" ¬
      buttons {"Ohne Kontakte", "Mit Kontakten"} ¬
      default button "Mit Kontakten" ¬
      with title "iPhone Call Export 0.1.3"
    set contactChoice to button returned of contactDialog

    set cmd to quoted form of cliPath & " --unlock --backup-root " & quoted form of backupPath & " --csv " & quoted form of outputPath
    if contactChoice is "Mit Kontakten" then set cmd to cmd & " --find-contacts"

    tell application "Terminal"
      activate
      do script cmd
    end tell
  on error errorMessage number errorNumber
    if errorNumber is -128 then return
    display alert "iPhone Call Export konnte nicht gestartet werden" ¬
      message (errorMessage & " (Fehler " & errorNumber & ")") ¬
      as critical ¬
      buttons {"OK"} ¬
      default button "OK"
  end try
end run
APPLESCRIPT

cat > "$MACOS_DIR/iphone-call-export-gui" <<'LAUNCHER'
#!/bin/zsh

HERE="$(cd "$(dirname "$0")" && pwd)"
CLI="$HERE/../Resources/iphone-call-export"
SCRIPT="$HERE/../Resources/launcher.applescript"
LOG_DIR="$HOME/Library/Logs/iPhone Call Export"
LOG_FILE="$LOG_DIR/launcher.log"
mkdir -p "$LOG_DIR"

{
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starte Oberfläche"
  echo "CLI: $CLI"
  echo "AppleScript: $SCRIPT"
} >> "$LOG_FILE"

if [[ ! -x "$CLI" ]]; then
  /usr/bin/osascript -e 'display alert "iPhone Call Export ist unvollständig installiert" message "Die Programmdatei fehlt oder ist nicht ausführbar. Bitte die App neu installieren." as critical'
  exit 1
fi

if [[ ! -f "$SCRIPT" ]]; then
  /usr/bin/osascript -e 'display alert "iPhone Call Export ist unvollständig installiert" message "Die Oberflächendatei fehlt. Bitte die App neu installieren." as critical'
  exit 1
fi

/usr/bin/osascript "$SCRIPT" "$CLI" >> "$LOG_FILE" 2>&1
STATUS=$?
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Oberfläche beendet, Status $STATUS" >> "$LOG_FILE"

if [[ $STATUS -ne 0 ]]; then
  /usr/bin/osascript -e 'display alert "iPhone Call Export konnte die Oberfläche nicht öffnen" message "Details stehen in ~/Library/Logs/iPhone Call Export/launcher.log" as critical'
fi

exit $STATUS
LAUNCHER
chmod 700 "$MACOS_DIR/iphone-call-export-gui"

mkdir -p "$INSTALL_DIR"
touch "$APP_DIR"

# Lokale, nicht signierte Entwicklungs-App ad-hoc signieren. Das stabilisiert den
# Start über LaunchServices; Fehler hier sollen die Installation nicht abbrechen.
/usr/bin/codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true

# LaunchServices soll die neue Bundle-Version sofort neu registrieren.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP_DIR" >/dev/null 2>&1 || true

printf '\n✓ Installiert: %s\n' "$APP_DIR"
echo "✓ App-Version: $APP_VERSION"
echo "Öffnen mit:"
echo "  open \"$APP_DIR\""
echo
echo "Der CSV-Dialog startet im Benutzerordner:"
echo "  $HOME"
echo
echo "Fehlerprotokoll:"
echo "  $HOME/Library/Logs/iPhone Call Export/launcher.log"
echo
echo "Hinweis: Terminal benötigt vollständigen Festplattenzugriff für MobileSync-Backups."
