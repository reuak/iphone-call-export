# Kommerzielle Lizenzierung

## Schutzmodell

Die App verwendet asymmetrisch signierte Offline-Lizenzen:

- Der private ECDSA-P-256-Schlüssel bleibt ausschließlich beim Hersteller.
- Die ausgelieferte App enthält nur den öffentlichen Schlüssel.
- Ein Lizenzcode besteht aus einer JSON-Nutzlast und deren Signatur.
- Die App akzeptiert nur Signaturen des eingebetteten öffentlichen Schlüssels.
- Aktivierte Lizenzen werden im macOS-Schlüsselbund gespeichert.
- Ohne gültige Lizenz sind Exporte mit höchstens 100 Einträgen möglich.

Dadurch gibt es keinen geheimen Mastercode und keinen im Programm eingebetteten symmetrischen Schlüssel, der einfach ausgelesen werden könnte.

## Wichtige Grenze

Kein rein clientseitiger Kopierschutz ist absolut manipulationssicher. Ein Angreifer mit Kontrolle über den Mac kann eine Binärdatei patchen. Für ein verkaufbares Produkt sollten deshalb zusätzlich eingesetzt werden:

1. signierte und notarisierte Release-Builds,
2. keine Verteilung des Quellcodes oder privaten Ausstellerschlüssels,
3. optional eine Aktivierungs-API mit Gerätebindung, Sperrliste und begrenzter Zahl von Aktivierungen,
4. regelmäßige Signatur- und Integritätsprüfungen an mehreren Stellen,
5. klare Lizenzbedingungen und Updateberechtigung.

Die aktuelle Implementierung ist ein belastbares Offline-MVP, keine DRM-Garantie.

## Lokale Entwicklungsumgebung

Beim ersten Installerlauf wird ein Ausstellerschlüssel erzeugt:

```text
~/.config/iphone-call-export-license/issuer-private.pem
```

Dieser Schlüssel darf niemals in ein App-Bundle, Installationspaket, Repository oder Kundenarchiv gelangen.

Lizenz ausstellen:

```bash
chmod +x scripts/issue-license.sh
./scripts/issue-license.sh "Muster GmbH"
```

Zeitlich befristete Lizenz:

```bash
./scripts/issue-license.sh "Muster GmbH" "2027-12-31T23:59:59Z"
```

Für Produktions-Releases muss derselbe öffentliche Schlüssel in allen ausgelieferten Builds verwendet werden. Der private Schlüssel sollte auf einem getrennten, gesicherten Build-/Lizenzsystem liegen.

## Backup-Automatisierung

Die App kann vorhandene lokale Finder-Backups erkennen und anschließend Import und Export automatisieren. Das erstmalige Vertrauen des Geräts, Einschalten der Backup-Verschlüsselung, Festlegen des Kennworts und Starten eines Backups erfolgen über den von Apple vorgesehenen Finder-Ablauf. Für diesen vollständigen Vorgang gibt es keine dokumentierte öffentliche macOS-API, auf die sich eine verkaufbare App stabil stützen sollte.
