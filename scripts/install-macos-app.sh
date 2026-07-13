#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="iPhone Call Export"
APP_VERSION="0.3.1"
INSTALL_DIR="${HOME}/Applications"
APP_DIR="${INSTALL_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

cd "$ROOT_DIR"
echo "Baue optimierte Export-Komponenten …"
rm -f target/release/iphone-call-export target/release/iphone-call-export-plain
cargo build --release -p iphone-call-export --bins

echo "Baue native macOS-Oberfläche …"
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp target/release/iphone-call-export "$RESOURCES_DIR/iphone-call-export-encrypted"
cp target/release/iphone-call-export-plain "$RESOURCES_DIR/iphone-call-export-plain"
chmod 700 "$RESOURCES_DIR/iphone-call-export-encrypted" "$RESOURCES_DIR/iphone-call-export-plain"

# Einheitlicher Starter: erkennt den Verschlüsselungsstatus des gewählten Backups
# und verwendet automatisch die passende lokale Export-Komponente.
cat > "$RESOURCES_DIR/iphone-call-export" <<'WRAPPER'
#!/bin/zsh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
backup_root=""
args=("$@")
for ((i = 1; i <= ${#args[@]}; i++)); do
  if [[ "${args[$i]}" == "--backup-root" ]] && (( i < ${#args[@]} )); then
    backup_root="${args[$((i + 1))]}"
    break
  fi
done

backup="$backup_root"
if [[ ! -f "$backup/Manifest.plist" && -d "$backup" ]]; then
  candidate="$(find "$backup" -mindepth 1 -maxdepth 1 -type d -exec test -f '{}/Manifest.plist' ';' -print 2>/dev/null | head -n 1)"
  [[ -n "$candidate" ]] && backup="$candidate"
fi

is_encrypted="true"
if [[ -f "$backup/Manifest.plist" ]]; then
  value="$(/usr/libexec/PlistBuddy -c 'Print :IsEncrypted' "$backup/Manifest.plist" 2>/dev/null || true)"
  [[ "$value" == "false" ]] && is_encrypted="false"
fi

if [[ "$is_encrypted" == "false" ]]; then
  exec "$HERE/iphone-call-export-plain" "$@"
else
  exec "$HERE/iphone-call-export-encrypted" "$@"
fi
WRAPPER
chmod 700 "$RESOURCES_DIR/iphone-call-export"

# Für unverschlüsselte Backups darf die Oberfläche kein Passwort verlangen.
# Die Quelländerung wird nur für den lokalen Build angewendet, damit ältere
# gespeicherte App-Daten und die übrige Oberfläche unverändert bleiben.
PATCHED_SWIFT="$(mktemp -t iphone-call-export-swift).swift"
trap 'rm -f "$PATCHED_SWIFT"' EXIT
python3 - "$ROOT_DIR/macos/IPhoneCallExportApp.swift" "$PATCHED_SWIFT" <<'PY'
from pathlib import Path
import sys
source = Path(sys.argv[1]).read_text()
source = source.replace(
    'guard !password.isEmpty else { presentError("Bitte das Backup-Passwort eingeben."); return }',
    'if backup.encrypted != false && password.isEmpty { presentError("Bitte das Backup-Passwort eingeben."); return }'
)
source = source.replace(
    'status = "Backup wird entschlüsselt und eingelesen …"',
    'status = backup.encrypted == false ? "Unverschlüsseltes Backup wird eingelesen …" : "Backup wird entschlüsselt und eingelesen …"'
)
source = source.replace(
    'SecureField("Backup-Passwort", text: $model.password)',
    'SecureField(model.selectedBackup?.encrypted == false ? "Kein Passwort erforderlich" : "Backup-Passwort", text: $model.password)\n                        .disabled(model.selectedBackup?.encrypted == false)'
)
Path(sys.argv[2]).write_text(source)
PY

xcrun swiftc \
  -parse-as-library \
  -O \
  -framework SwiftUI \
  -framework AppKit \
  "$PATCHED_SWIFT" \
  -o "$MACOS_DIR/iPhone Call Export"
chmod 700 "$MACOS_DIR/iPhone Call Export"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
  <key>CFBundleExecutable</key><string>iPhone Call Export</string>
  <key>CFBundleIdentifier</key><string>de.reuak.iphone-call-export</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${APP_VERSION}</string>
  <key>CFBundleVersion</key><string>31</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

mkdir -p "$INSTALL_DIR"
touch "$APP_DIR"
/usr/bin/codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP_DIR" >/dev/null 2>&1 || true

printf '\n✓ Installiert: %s\n' "$APP_DIR"
echo "✓ App-Version: $APP_VERSION"
echo "Öffnen mit:"
echo "  open \"$APP_DIR\""
echo
echo "Verschlüsselte und unverschlüsselte Finder-Backups werden automatisch erkannt."
echo "Bei unverschlüsselten Backups ist kein Passwort erforderlich."
echo
echo "Die App speichert die eingelesene Telefonliste dauerhaft unter:"
echo "  $HOME/Library/Application Support/iPhone Call Export/telefonate.json"
echo
echo "Backup-Passwort und entschlüsselte Datenbanken werden nicht dauerhaft gespeichert."
echo
echo "Hinweis: Für den automatischen Zugriff auf MobileSync-Backups kann die App"
echo "unter Systemeinstellungen → Datenschutz & Sicherheit → Vollständiger Festplattenzugriff"
echo "freigegeben werden. Alternativ kann der Backup-Ordner in der App ausgewählt werden."
