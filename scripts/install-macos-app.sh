#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="iPhone Call Export"
APP_VERSION="0.4.0"
INSTALL_DIR="${HOME}/Applications"
APP_DIR="${INSTALL_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
LICENSE_DIR="${HOME}/.config/iphone-call-export-license"
PRIVATE_KEY="${LICENSE_DIR}/issuer-private.pem"
PUBLIC_KEY_PEM="${LICENSE_DIR}/issuer-public.pem"
PUBLIC_KEY_DER="${LICENSE_DIR}/issuer-public.der"

cd "$ROOT_DIR"
echo "Baue optimierte Export-Komponenten …"
rm -f target/release/iphone-call-export target/release/iphone-call-export-plain
cargo build --release -p iphone-call-export --bins

echo "Bereite Lizenzsignatur vor …"
mkdir -p "$LICENSE_DIR"
chmod 700 "$LICENSE_DIR"
if [[ ! -f "$PRIVATE_KEY" ]]; then
  /usr/bin/openssl ecparam -name prime256v1 -genkey -noout -out "$PRIVATE_KEY"
  chmod 600 "$PRIVATE_KEY"
  echo "✓ Neuer privater Ausstellerschlüssel wurde nur lokal erzeugt: $PRIVATE_KEY"
fi
/usr/bin/openssl ec -in "$PRIVATE_KEY" -pubout -out "$PUBLIC_KEY_PEM" >/dev/null 2>&1
/usr/bin/openssl pkey -pubin -in "$PUBLIC_KEY_PEM" -outform DER -out "$PUBLIC_KEY_DER"
chmod 600 "$PUBLIC_KEY_PEM" "$PUBLIC_KEY_DER"

echo "Baue native macOS-Oberfläche …"
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp target/release/iphone-call-export "$RESOURCES_DIR/iphone-call-export-encrypted"
cp target/release/iphone-call-export-plain "$RESOURCES_DIR/iphone-call-export-plain"
cp "$PUBLIC_KEY_DER" "$RESOURCES_DIR/license-public-key.der"
chmod 700 "$RESOURCES_DIR/iphone-call-export-encrypted" "$RESOURCES_DIR/iphone-call-export-plain"
chmod 600 "$RESOURCES_DIR/license-public-key.der"

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
source = source.replace(
    'let rows = filteredCalls\n        guard !rows.isEmpty else { presentError("Die gefilterte Ansicht enthält keine Telefonate."); return }',
    'let rows = filteredCalls\n        guard !rows.isEmpty else { presentError("Die gefilterte Ansicht enthält keine Telefonate."); return }\n        if rows.count > 100 && !LicenseService.shared.isLicensed {\n            presentError("Die Testversion exportiert höchstens 100 Einträge. Bitte enger filtern oder einen gültigen Lizenzcode aktivieren.")\n            return\n        }'
)
source = source.replace('@main\nstruct IPhoneCallExportApp: App {', 'struct LegacyIPhoneCallExportApp: App {')
old_pdf = '''        let operation = NSPrintOperation.pdfOperation(with: textView, inside: textView.bounds, to: url, printInfo: printInfo)
        operation.showsPrintPanel = false
        operation.showsProgressPanel = false
        guard operation.run() else { throw NSError(domain: "PDF", code: 1, userInfo: [NSLocalizedDescriptionKey: "PDF konnte nicht erzeugt werden"]) }
'''
new_pdf = '''        let pdfData = NSMutableData()
        let operation = NSPrintOperation.pdfOperation(with: textView, inside: textView.bounds, to: pdfData, printInfo: printInfo)
        operation.showsPrintPanel = false
        operation.showsProgressPanel = false
        guard operation.run() else {
            throw NSError(domain: "PDF", code: 1, userInfo: [NSLocalizedDescriptionKey: "PDF konnte nicht erzeugt werden"])
        }
        guard pdfData.write(to: url, atomically: true) else {
            throw NSError(domain: "PDF", code: 2, userInfo: [NSLocalizedDescriptionKey: "PDF-Datei konnte nicht gespeichert werden"])
        }
'''
if old_pdf not in source:
    raise SystemExit("PDF-Code konnte im Swift-Quelltext nicht gefunden werden")
source = source.replace(old_pdf, new_pdf)
Path(sys.argv[2]).write_text(source)
PY

xcrun swiftc \
  -parse-as-library \
  -O \
  -framework SwiftUI \
  -framework AppKit \
  -framework CryptoKit \
  -framework Security \
  "$PATCHED_SWIFT" \
  "$ROOT_DIR/macos/CommercialShell.swift" \
  -o "$MACOS_DIR/iPhone Call Export"
chmod 700 "$MACOS_DIR/iPhone Call Export"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
  <key>CFBundleExecutable</key><string>iPhone Call Export</string>
  <key>CFBundleIdentifier</key><string>de.reuak.iphone-call-export</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${APP_VERSION}</string>
  <key>CFBundleVersion</key><string>40</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

mkdir -p "$INSTALL_DIR"
touch "$APP_DIR"
/usr/bin/codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP_DIR" >/dev/null 2>&1 || true

printf '\n✓ Installiert: %s\n' "$APP_DIR"
echo "✓ App-Version: $APP_VERSION"
echo "✓ Testversion: höchstens 100 Einträge je Export"
echo "✓ Lizenzcodes werden asymmetrisch signiert und im macOS-Schlüsselbund gespeichert"
echo
echo "Öffnen mit:"
echo "  open \"$APP_DIR\""
echo
echo "Lizenz ausstellen mit:"
echo "  ./scripts/issue-license.sh \"Kundenname\""
echo
echo "Wichtig für den Verkauf: Nur signierte/notarisierte Binärpakete verteilen; den privaten"
echo "Ausstellerschlüssel niemals mit der App oder dem Quellpaket ausliefern."
