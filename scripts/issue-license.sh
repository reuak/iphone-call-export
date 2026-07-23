#!/bin/zsh
set -euo pipefail

CUSTOMER="${1:-}"
EXPIRES_AT="${2:-}"
LICENSE_DIR="${HOME}/.config/iphone-call-export-license"
PRIVATE_KEY="${LICENSE_DIR}/issuer-private.pem"

if [[ -z "$CUSTOMER" ]]; then
  echo "Verwendung: $0 \"Kundenname\" [Ablaufdatum, z. B. 2027-12-31T23:59:59Z]" >&2
  exit 2
fi
if [[ ! -f "$PRIVATE_KEY" ]]; then
  echo "Privater Ausstellerschlüssel fehlt: $PRIVATE_KEY" >&2
  echo "Bitte zuerst scripts/install-macos-app.sh ausführen." >&2
  exit 1
fi

TMP_DIR="$(mktemp -d -t iphone-call-export-license)"
trap 'rm -rf "$TMP_DIR"' EXIT
PAYLOAD="$TMP_DIR/payload.json"
SIGNATURE="$TMP_DIR/signature.der"

python3 - "$CUSTOMER" "$EXPIRES_AT" > "$PAYLOAD" <<'PY'
import datetime
import json
import sys
import uuid

customer = sys.argv[1]
expires = sys.argv[2] or None
payload = {
    "version": 1,
    "licenseID": str(uuid.uuid4()),
    "customer": customer,
    "issuedAt": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    "expiresAt": expires,
    "features": ["unlimited-export"],
}
print(json.dumps(payload, ensure_ascii=False, separators=(",", ":")), end="")
PY

/usr/bin/openssl dgst -sha256 -sign "$PRIVATE_KEY" -out "$SIGNATURE" "$PAYLOAD"

CODE="$(python3 - "$PAYLOAD" "$SIGNATURE" <<'PY'
import base64
import pathlib
import sys

def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).decode("ascii").rstrip("=")

payload = pathlib.Path(sys.argv[1]).read_bytes()
signature = pathlib.Path(sys.argv[2]).read_bytes()
print(f"{b64url(payload)}.{b64url(signature)}")
PY
)"

printf '\nLizenz für: %s\n\n%s\n\n' "$CUSTOMER" "$CODE"
echo "Der private Schlüssel bleibt lokal unter:"
echo "  $PRIVATE_KEY"
echo "Diesen Schlüssel niemals an Kunden weitergeben oder in das Repository einchecken."
