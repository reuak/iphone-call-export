#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="iPhone Call Export"
APP_VERSION="0.3.0"
INSTALL_DIR="${HOME}/Applications"
APP_DIR="${INSTALL_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

cd "$ROOT_DIR"
echo "Baue optimierte Export-Komponente …"
rm -f target/release/iphone-call-export
cargo build --release -p iphone-call-export

echo "Baue native macOS-Oberfläche …"
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp target/release/iphone-call-export "$RESOURCES_DIR/iphone-call-export"
chmod 700 "$RESOURCES_DIR/iphone-call-export"

xcrun swiftc \
  -parse-as-library \
  -O \
  -framework SwiftUI \
  -framework AppKit \
  "$ROOT_DIR/macos/IPhoneCallExportApp.swift" \
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
  <key>CFBundleVersion</key><string>30</string>
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
echo "Die App speichert die eingelesene Telefonliste dauerhaft unter:"
echo "  $HOME/Library/Application Support/iPhone Call Export/telefonate.json"
echo
echo "Backup-Passwort und entschlüsselte Datenbanken werden nicht dauerhaft gespeichert."
echo
echo "Hinweis: Für den automatischen Zugriff auf MobileSync-Backups kann die App"
echo "unter Systemeinstellungen → Datenschutz & Sicherheit → Vollständiger Festplattenzugriff"
echo "freigegeben werden. Alternativ kann der Backup-Ordner in der App ausgewählt werden."
