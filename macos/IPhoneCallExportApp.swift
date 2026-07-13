import SwiftUI
import AppKit

struct BackupItem: Identifiable, Hashable {
    let id = UUID()
    let path: URL
    let deviceName: String
    let iosVersion: String
    let encrypted: Bool?
    let modified: Date
    let deviceIdentifier: String

    var title: String { deviceName.isEmpty ? path.lastPathComponent : deviceName }

    var subtitle: String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        let encryption = encrypted == true ? "verschlüsselt" : (encrypted == false ? "nicht verschlüsselt" : "Verschlüsselung unbekannt")
        return "iOS \(iosVersion.isEmpty ? "unbekannt" : iosVersion) · \(encryption) · \(formatter.string(from: modified))"
    }
}

struct StoredCall: Identifiable, Codable, Hashable {
    var id: String
    var deviceIdentifier: String
    var deviceName: String
    var date: String
    var time: String
    var timezone: String
    var durationSeconds: String
    var durationMinutes: String
    var direction: String
    var answered: String
    var phone: String
    var name: String
    var callListName: String
    var organization: String
    var contactSource: String
    var callType: String
    var country: String
    var provider: String
    var customer: String = ""
    var project: String = ""
    var note: String = ""
    var importedAt: Date = Date()
}

struct ImportState: Codable {
    var deviceIdentifier: String
    var deviceName: String
    var backupPath: String
    var backupModified: Date
    var importedAt: Date
}

struct PersistentStore: Codable {
    var calls: [StoredCall] = []
    var imports: [ImportState] = []
}

@MainActor
final class AppModel: ObservableObject {
    @Published var backups: [BackupItem] = []
    @Published var selectedBackup: BackupItem?
    @Published var password = ""
    @Published var matchContacts = true
    @Published var isRunning = false
    @Published var status = "Bereit"
    @Published var log = ""
    @Published var showError = false
    @Published var errorMessage = ""
    @Published var showDifferentDeviceWarning = false
    @Published var pendingDifferentDevice: BackupItem?

    @Published var calls: [StoredCall] = []
    @Published var selectedCallID: String?
    @Published var searchText = ""
    @Published var dateFilter = ""
    @Published var nameFilter = ""
    @Published var phoneFilter = ""
    @Published var customerFilter = ""
    @Published var projectFilter = ""
    @Published var directionFilter = "Alle"

    private var process: Process?
    private var store = PersistentStore()

    init() {
        loadStore()
        refreshBackups()
    }

    var filteredCalls: [StoredCall] {
        calls.filter { call in
            contains(call.date, dateFilter) &&
            contains(call.name, nameFilter) &&
            contains(call.phone, phoneFilter) &&
            contains(call.customer, customerFilter) &&
            contains(call.project, projectFilter) &&
            (directionFilter == "Alle" || call.direction == directionFilter) &&
            (searchText.isEmpty || [call.date, call.time, call.name, call.phone, call.customer, call.project, call.note, call.organization]
                .joined(separator: " ").localizedCaseInsensitiveContains(searchText))
        }
        .sorted { lhs, rhs in
            let left = lhs.date + " " + lhs.time
            let right = rhs.date + " " + rhs.time
            return left > right
        }
    }

    var selectedCall: StoredCall? {
        guard let selectedCallID else { return nil }
        return calls.first(where: { $0.id == selectedCallID })
    }

    func refreshBackups() {
        let root = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/MobileSync/Backup", isDirectory: true)
        backups = scanBackups(in: root)
        if selectedBackup == nil || !backups.contains(where: { $0.deviceIdentifier == selectedBackup?.deviceIdentifier && $0.path == selectedBackup?.path }) {
            selectedBackup = backups.first
        }

        if let newest = backups.first {
            let previous = store.imports
                .filter { $0.deviceIdentifier == newest.deviceIdentifier }
                .max(by: { $0.backupModified < $1.backupModified })
            if previous == nil {
                status = calls.isEmpty ? "Neues Backup gefunden – zum Einlesen Passwort eingeben" : "Backup eines noch nicht importierten Geräts gefunden"
            } else if newest.modified > (previous?.backupModified ?? .distantPast) {
                status = "Neueres Backup gefunden – neue Anrufe können ergänzt werden"
            } else {
                status = "\(calls.count) gespeicherte Telefonate · Backup ist aktuell"
            }
        } else {
            status = calls.isEmpty ? "Keine lokalen Finder-Backups gefunden" : "\(calls.count) gespeicherte Telefonate geladen"
        }
    }

    func chooseBackupFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Backup auswählen"
        panel.message = "Wähle den MobileSync/Backup-Ordner oder einen konkreten Geräte-Backup-Ordner."
        if panel.runModal() == .OK, let url = panel.url {
            let found = scanBackups(in: url)
            if found.isEmpty, let item = readBackup(at: url) {
                backups = [item]
                selectedBackup = item
            } else if !found.isEmpty {
                backups = found
                selectedBackup = found.first
            } else {
                presentError("Im ausgewählten Ordner wurde kein gültiges iPhone-Backup mit Manifest.plist gefunden.")
            }
        }
    }

    func importSelectedBackup(forceDifferentDevice: Bool = false) {
        guard let backup = selectedBackup else { presentError("Bitte zuerst ein Backup auswählen."); return }
        guard !password.isEmpty else { presentError("Bitte das Backup-Passwort eingeben."); return }

        let knownDevices = Set(store.imports.map(\.deviceIdentifier).filter { !$0.isEmpty })
        if !forceDifferentDevice && !knownDevices.isEmpty && !knownDevices.contains(backup.deviceIdentifier) {
            pendingDifferentDevice = backup
            showDifferentDeviceWarning = true
            return
        }
        startImport(backup)
    }

    func confirmDifferentDeviceImport() {
        showDifferentDeviceWarning = false
        importSelectedBackup(forceDifferentDevice: true)
    }

    func updateSelectedAssignment(customer: String? = nil, project: String? = nil, note: String? = nil) {
        guard let id = selectedCallID, let index = calls.firstIndex(where: { $0.id == id }) else { return }
        if let customer { calls[index].customer = customer }
        if let project { calls[index].project = project }
        if let note { calls[index].note = note }
        store.calls = calls
        saveStore()
    }

    func exportFiltered(format: ExportFormat) {
        let rows = filteredCalls
        guard !rows.isEmpty else { presentError("Die gefilterte Ansicht enthält keine Telefonate."); return }
        let panel = NSSavePanel()
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        panel.nameFieldStringValue = "telefonate-gefiltert.\(format.fileExtension)"
        panel.prompt = "Exportieren"
        if panel.runModal() != .OK || panel.url == nil { return }
        var url = panel.url!
        if url.pathExtension.lowercased() != format.fileExtension { url.appendPathExtension(format.fileExtension) }
        do {
            switch format {
            case .csv: try writeCSV(rows, to: url)
            case .excel: try writeSpreadsheetML(rows, to: url)
            case .pdf: try writePDF(rows, to: url)
            }
            status = "\(rows.count) gefilterte Telefonate exportiert"
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } catch {
            presentError("Export fehlgeschlagen: \(error.localizedDescription)")
        }
    }

    private func startImport(_ backup: BackupItem) {
        guard let cli = Bundle.main.url(forResource: "iphone-call-export", withExtension: nil) else {
            presentError("Die eingebettete Export-Komponente fehlt. Bitte die App neu installieren.")
            return
        }

        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("iphone-call-export-\(UUID().uuidString).csv")
        isRunning = true
        status = "Backup wird entschlüsselt und eingelesen …"
        log = ""

        let task = Process()
        task.executableURL = cli
        var args = ["--unlock", "--password-stdin", "--backup-root", backup.path.path, "--csv", tempURL.path]
        if matchContacts { args.append("--find-contacts") }
        task.arguments = args

        let input = Pipe()
        let output = Pipe()
        task.standardInput = input
        task.standardOutput = output
        task.standardError = output

        output.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty, let text = String(data: data, encoding: .utf8) else { return }
            Task { @MainActor in self?.log.append(text) }
        }

        task.terminationHandler = { [weak self] process in
            Task { @MainActor in
                output.fileHandleForReading.readabilityHandler = nil
                self?.isRunning = false
                self?.password = ""
                defer { try? FileManager.default.removeItem(at: tempURL); self?.process = nil }
                guard process.terminationStatus == 0 else {
                    self?.status = "Import fehlgeschlagen"
                    self?.presentError("Das Backup konnte nicht eingelesen werden. Details stehen im Protokoll.")
                    return
                }
                do {
                    let imported = try self?.readExportCSV(tempURL, backup: backup) ?? []
                    let added = self?.merge(imported) ?? 0
                    self?.store.imports.append(ImportState(
                        deviceIdentifier: backup.deviceIdentifier,
                        deviceName: backup.deviceName,
                        backupPath: backup.path.path,
                        backupModified: backup.modified,
                        importedAt: Date()
                    ))
                    self?.saveStore()
                    self?.status = added == 0 ? "Keine neuen Telefonate gefunden" : "\(added) neue Telefonate ergänzt"
                } catch {
                    self?.presentError("Die exportierten Daten konnten nicht übernommen werden: \(error.localizedDescription)")
                }
            }
        }

        do {
            try task.run()
            process = task
            input.fileHandleForWriting.write(Data((password + "\n").utf8))
            try? input.fileHandleForWriting.close()
        } catch {
            isRunning = false
            presentError("Die Export-Komponente konnte nicht gestartet werden: \(error.localizedDescription)")
        }
    }

    private func merge(_ imported: [StoredCall]) -> Int {
        var existing = Dictionary(uniqueKeysWithValues: calls.map { ($0.id, $0) })
        var added = 0
        for call in imported {
            if let old = existing[call.id] {
                var merged = call
                merged.customer = old.customer
                merged.project = old.project
                merged.note = old.note
                existing[call.id] = merged
            } else {
                existing[call.id] = call
                added += 1
            }
        }
        calls = Array(existing.values)
        store.calls = calls
        return added
    }

    private func readExportCSV(_ url: URL, backup: BackupItem) throws -> [StoredCall] {
        let text = try String(contentsOf: url, encoding: .utf8)
        let records = parseDelimited(text, delimiter: ";")
        guard let header = records.first else { return [] }
        let index = Dictionary(uniqueKeysWithValues: header.enumerated().map { ($0.element, $0.offset) })
        func value(_ row: [String], _ name: String) -> String {
            guard let i = index[name], i < row.count else { return "" }
            return row[i]
        }
        return records.dropFirst().filter { !$0.isEmpty }.map { row in
            let unique = value(row, "Eindeutige_ID")
            let fallback = [backup.deviceIdentifier, value(row, "Datum"), value(row, "Zeit"), value(row, "Rufnummer")].joined(separator: "|")
            return StoredCall(
                id: unique.isEmpty ? fallback : backup.deviceIdentifier + "|" + unique,
                deviceIdentifier: backup.deviceIdentifier,
                deviceName: backup.deviceName,
                date: value(row, "Datum"), time: value(row, "Zeit"), timezone: value(row, "Zeitzone"),
                durationSeconds: value(row, "Dauer_Sekunden"), durationMinutes: value(row, "Dauer_Minuten"),
                direction: value(row, "Richtung"), answered: value(row, "Angenommen"), phone: value(row, "Rufnummer"),
                name: value(row, "Name"), callListName: value(row, "Name_Anrufliste"),
                organization: value(row, "Kontakt_Organisation"), contactSource: value(row, "Kontaktquelle"),
                callType: value(row, "Anruftyp"), country: value(row, "Land"), provider: value(row, "Dienstanbieter")
            )
        }
    }

    private func parseDelimited(_ text: String, delimiter: Character) -> [[String]] {
        var rows: [[String]] = []
        var row: [String] = []
        var field = ""
        var quoted = false
        var iterator = text.makeIterator()
        while let char = iterator.next() {
            if quoted {
                if char == "\"" {
                    quoted = false
                } else { field.append(char) }
            } else if char == "\"" {
                quoted = true
            } else if char == delimiter {
                row.append(field); field = ""
            } else if char == "\n" {
                row.append(field); rows.append(row); row = []; field = ""
            } else if char != "\r" {
                field.append(char)
            }
        }
        if !field.isEmpty || !row.isEmpty { row.append(field); rows.append(row) }
        return rows
    }

    private func writeCSV(_ rows: [StoredCall], to url: URL) throws {
        let headers = exportHeaders
        var lines = [headers.map(csvEscape).joined(separator: ";")]
        lines += rows.map { exportValues($0).map(csvEscape).joined(separator: ";") }
        try (lines.joined(separator: "\n") + "\n").write(to: url, atomically: true, encoding: .utf8)
    }

    private func writeSpreadsheetML(_ rows: [StoredCall], to url: URL) throws {
        func cell(_ value: String) -> String { "<Cell><Data ss:Type=\"String\">\(xmlEscape(value))</Data></Cell>" }
        let header = exportHeaders.map(cell).joined()
        let body = rows.map { "<Row>\(exportValues($0).map(cell).joined())</Row>" }.joined()
        let xml = """
        <?xml version="1.0"?>
        <?mso-application progid="Excel.Sheet"?>
        <Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet" xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet">
          <Worksheet ss:Name="Telefonate"><Table><Row>\(header)</Row>\(body)</Table></Worksheet>
        </Workbook>
        """
        try xml.write(to: url, atomically: true, encoding: .utf8)
    }

    private func writePDF(_ rows: [StoredCall], to url: URL) throws {
        let title = "Telefonate – gefilterte Ansicht\n\n"
        let header = "Datum\tZeit\tMin.\tName\tRufnummer\tKunde\tProjekt\n"
        let body = rows.map { "\($0.date)\t\($0.time)\t\($0.durationMinutes)\t\($0.name)\t\($0.phone)\t\($0.customer)\t\($0.project)" }.joined(separator: "\n")
        let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 1100, height: max(800, CGFloat(rows.count * 18 + 100))))
        textView.string = title + header + body
        textView.font = NSFont.monospacedSystemFont(ofSize: 9, weight: .regular)
        let printInfo = NSPrintInfo.shared.copy() as! NSPrintInfo
        printInfo.orientation = .landscape
        let operation = NSPrintOperation.pdfOperation(with: textView, inside: textView.bounds, to: url, printInfo: printInfo)
        operation.showsPrintPanel = false
        operation.showsProgressPanel = false
        guard operation.run() else { throw NSError(domain: "PDF", code: 1, userInfo: [NSLocalizedDescriptionKey: "PDF konnte nicht erzeugt werden"]) }
    }

    private var exportHeaders: [String] {
        ["Datum", "Zeit", "Zeitzone", "Dauer_Sekunden", "Dauer_Minuten", "Richtung", "Angenommen", "Rufnummer", "Name", "Organisation", "Kunde", "Projekt", "Notiz", "Gerät"]
    }

    private func exportValues(_ c: StoredCall) -> [String] {
        [c.date, c.time, c.timezone, c.durationSeconds, c.durationMinutes, c.direction, c.answered, c.phone, c.name, c.organization, c.customer, c.project, c.note, c.deviceName]
    }

    private func csvEscape(_ value: String) -> String {
        if value.contains(";") || value.contains("\"") || value.contains("\n") { return "\"" + value.replacingOccurrences(of: "\"", with: "\"\"") + "\"" }
        return value
    }

    private func xmlEscape(_ value: String) -> String {
        value.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;").replacingOccurrences(of: "\"", with: "&quot;")
    }

    private func contains(_ value: String, _ filter: String) -> Bool { filter.isEmpty || value.localizedCaseInsensitiveContains(filter) }

    private var storeURL: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("iPhone Call Export", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("telefonate.json")
    }

    private func loadStore() {
        guard let data = try? Data(contentsOf: storeURL), let decoded = try? JSONDecoder().decode(PersistentStore.self, from: data) else { return }
        store = decoded
        calls = decoded.calls
    }

    private func saveStore() {
        store.calls = calls
        guard let data = try? JSONEncoder().encode(store) else { return }
        try? data.write(to: storeURL, options: .atomic)
    }

    private func scanBackups(in root: URL) -> [BackupItem] {
        if let direct = readBackup(at: root) { return [direct] }
        guard let children = try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey, .isDirectoryKey], options: [.skipsHiddenFiles]) else { return [] }
        return children.compactMap(readBackup(at:)).sorted { $0.modified > $1.modified }
    }

    private func readBackup(at url: URL) -> BackupItem? {
        let manifestURL = url.appendingPathComponent("Manifest.plist")
        let infoURL = url.appendingPathComponent("Info.plist")
        guard FileManager.default.fileExists(atPath: manifestURL.path), let infoData = try? Data(contentsOf: infoURL), let info = try? PropertyListSerialization.propertyList(from: infoData, options: [], format: nil) as? [String: Any] else { return nil }
        var encrypted: Bool?
        if let manifestData = try? Data(contentsOf: manifestURL), let manifest = try? PropertyListSerialization.propertyList(from: manifestData, options: [], format: nil) as? [String: Any] { encrypted = manifest["IsEncrypted"] as? Bool }
        let values = try? url.resourceValues(forKeys: [.contentModificationDateKey])
        let identifier = (info["Unique Identifier"] as? String) ?? (info["Target Identifier"] as? String) ?? url.lastPathComponent
        return BackupItem(path: url, deviceName: info["Device Name"] as? String ?? "", iosVersion: info["Product Version"] as? String ?? "", encrypted: encrypted, modified: values?.contentModificationDate ?? .distantPast, deviceIdentifier: identifier)
    }

    private func presentError(_ message: String) { errorMessage = message; showError = true }
}

enum ExportFormat: String, CaseIterable, Identifiable {
    case csv = "CSV"
    case excel = "Excel"
    case pdf = "PDF"
    var id: String { rawValue }
    var fileExtension: String { self == .excel ? "xls" : rawValue.lowercased() }
}

struct ContentView: View {
    @StateObject private var model = AppModel()
    @State private var exportFormat: ExportFormat = .csv

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            importBar
            Divider()
            filterBar
            Divider()
            HSplitView {
                callTable
                detailEditor
                    .frame(minWidth: 260, idealWidth: 300, maxWidth: 360)
            }
            Divider()
            footer
        }
        .frame(minWidth: 1150, minHeight: 700)
        .alert("iPhone Call Export", isPresented: $model.showError) { Button("OK", role: .cancel) {} } message: { Text(model.errorMessage) }
        .alert("Anderes iPhone erkannt", isPresented: $model.showDifferentDeviceWarning) {
            Button("Abbrechen", role: .cancel) {}
            Button("Als weiteres Gerät importieren") { model.confirmDifferentDeviceImport() }
        } message: {
            Text("Das ausgewählte Backup gehört nicht zu einem bisher importierten Gerät. Die Anrufe werden getrennt über die Geräte-ID gespeichert und nicht mit einem anderen iPhone verwechselt.")
        }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text("Telefonate").font(.title.bold())
                Text("Dauerhafte Anrufliste mit Kunden- und Projektzuordnung").foregroundStyle(.secondary)
            }
            Spacer()
            Text("\(model.filteredCalls.count) von \(model.calls.count)").monospacedDigit()
        }.padding(16)
    }

    private var importBar: some View {
        HStack(spacing: 10) {
            Picker("Backup", selection: $model.selectedBackup) {
                ForEach(model.backups) { backup in Text("\(backup.title) · \(backup.subtitle)").tag(Optional(backup)) }
            }.frame(minWidth: 350)
            Button("Ordner …") { model.chooseBackupFolder() }
            Button("Prüfen") { model.refreshBackups() }
            Toggle("Kontakte", isOn: $model.matchContacts).toggleStyle(.checkbox)
            SecureField("Backup-Passwort", text: $model.password).frame(width: 180)
            Button(model.isRunning ? "Läuft …" : "Backup einlesen") { model.importSelectedBackup() }
                .disabled(model.isRunning || model.selectedBackup == nil)
            if model.isRunning { ProgressView().controlSize(.small) }
        }.padding(12)
    }

    private var filterBar: some View {
        VStack(spacing: 8) {
            HStack {
                TextField("Alles durchsuchen", text: $model.searchText)
                TextField("Datum", text: $model.dateFilter).frame(width: 110)
                TextField("Name", text: $model.nameFilter).frame(width: 150)
                TextField("Rufnummer", text: $model.phoneFilter).frame(width: 140)
                TextField("Kunde", text: $model.customerFilter).frame(width: 140)
                TextField("Projekt", text: $model.projectFilter).frame(width: 140)
                Picker("Richtung", selection: $model.directionFilter) {
                    Text("Alle").tag("Alle"); Text("eingehend").tag("eingehend"); Text("ausgehend").tag("ausgehend")
                }.frame(width: 130)
            }
            HStack {
                Spacer()
                Picker("Format", selection: $exportFormat) { ForEach(ExportFormat.allCases) { Text($0.rawValue).tag($0) } }.frame(width: 120)
                Button("Gefilterte Ansicht exportieren …") { model.exportFiltered(format: exportFormat) }
            }
        }.padding(10)
    }

    private var callTable: some View {
        Table(model.filteredCalls, selection: $model.selectedCallID) {
            TableColumn("Datum", value: \.date).width(min: 90, ideal: 100)
            TableColumn("Zeit", value: \.time).width(min: 70, ideal: 80)
            TableColumn("Min.", value: \.durationMinutes).width(min: 55, ideal: 65)
            TableColumn("Richtung", value: \.direction).width(min: 80, ideal: 90)
            TableColumn("Name", value: \.name).width(min: 140, ideal: 190)
            TableColumn("Rufnummer", value: \.phone).width(min: 120, ideal: 145)
            TableColumn("Kunde", value: \.customer).width(min: 110, ideal: 150)
            TableColumn("Projekt", value: \.project).width(min: 110, ideal: 150)
            TableColumn("Gerät", value: \.deviceName).width(min: 90, ideal: 120)
        }
    }

    private var detailEditor: some View {
        Group {
            if let call = model.selectedCall {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Zuordnung").font(.headline)
                    Text(call.name.isEmpty ? call.phone : call.name).font(.title3.bold())
                    Text("\(call.date) · \(call.time) · \(call.durationMinutes) Min.").foregroundStyle(.secondary)
                    Divider()
                    Text("Kunde").font(.caption).foregroundStyle(.secondary)
                    TextField("Kunde", text: Binding(get: { model.selectedCall?.customer ?? "" }, set: { model.updateSelectedAssignment(customer: $0) }))
                    Text("Projekt").font(.caption).foregroundStyle(.secondary)
                    TextField("Projekt", text: Binding(get: { model.selectedCall?.project ?? "" }, set: { model.updateSelectedAssignment(project: $0) }))
                    Text("Notiz").font(.caption).foregroundStyle(.secondary)
                    TextEditor(text: Binding(get: { model.selectedCall?.note ?? "" }, set: { model.updateSelectedAssignment(note: $0) })).frame(minHeight: 100)
                    Divider()
                    Text(call.phone).textSelection(.enabled)
                    Text(call.organization).foregroundStyle(.secondary)
                    Spacer()
                }.padding(14)
            } else {
                ContentUnavailableView("Kein Telefonat ausgewählt", systemImage: "phone", description: Text("Wähle eine Zeile aus, um Kunde und Projekt zuzuordnen."))
            }
        }
    }

    private var footer: some View {
        HStack {
            Text(model.status).foregroundStyle(.secondary)
            Spacer()
            DisclosureGroup("Protokoll") {
                ScrollView { Text(model.log.isEmpty ? "Noch keine Ausgabe." : model.log).font(.caption.monospaced()).textSelection(.enabled).frame(maxWidth: 520, alignment: .leading) }
                    .frame(width: 520, height: 120)
            }.frame(width: 620)
        }.padding(10)
    }
}

@main
struct IPhoneCallExportApp: App {
    var body: some Scene {
        WindowGroup { ContentView() }
        .windowStyle(.titleBar)
    }
}
