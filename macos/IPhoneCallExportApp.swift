import SwiftUI
import AppKit

struct BackupItem: Identifiable, Hashable {
    let id = UUID()
    let path: URL
    let deviceName: String
    let iosVersion: String
    let encrypted: Bool?
    let modified: Date

    var title: String {
        deviceName.isEmpty ? path.lastPathComponent : deviceName
    }

    var subtitle: String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        let encryption = encrypted == true ? "verschlüsselt" : (encrypted == false ? "nicht verschlüsselt" : "Verschlüsselung unbekannt")
        return "iOS \(iosVersion.isEmpty ? "unbekannt" : iosVersion) · \(encryption) · \(formatter.string(from: modified))"
    }
}

@MainActor
final class AppModel: ObservableObject {
    @Published var backups: [BackupItem] = []
    @Published var selectedBackup: BackupItem?
    @Published var outputURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("iphone-anrufe-mit-kontakten.csv")
    @Published var password = ""
    @Published var matchContacts = true
    @Published var isRunning = false
    @Published var status = "Bereit"
    @Published var log = ""
    @Published var showError = false
    @Published var errorMessage = ""

    private var process: Process?

    init() {
        refreshBackups()
    }

    func refreshBackups() {
        let root = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/MobileSync/Backup", isDirectory: true)
        backups = scanBackups(in: root)
        selectedBackup = backups.first
        status = backups.isEmpty ? "Keine lokalen Finder-Backups gefunden" : "\(backups.count) Backup(s) gefunden"
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
            if found.isEmpty, FileManager.default.fileExists(atPath: url.appendingPathComponent("Manifest.plist").path) {
                if let item = readBackup(at: url) {
                    backups = [item]
                    selectedBackup = item
                }
            } else if !found.isEmpty {
                backups = found
                selectedBackup = found.first
            } else {
                presentError("Im ausgewählten Ordner wurde kein gültiges iPhone-Backup mit Manifest.plist gefunden.")
            }
        }
    }

    func chooseOutput() {
        let panel = NSSavePanel()
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        panel.nameFieldStringValue = outputURL.lastPathComponent
        panel.allowedContentTypes = [.commaSeparatedText]
        panel.prompt = "Speichern"
        if panel.runModal() == .OK, let url = panel.url {
            outputURL = url.pathExtension.lowercased() == "csv" ? url : url.appendingPathExtension("csv")
        }
    }

    func startExport() {
        guard let backup = selectedBackup else {
            presentError("Bitte zuerst ein Backup auswählen.")
            return
        }
        guard !password.isEmpty else {
            presentError("Bitte das Backup-Passwort eingeben.")
            return
        }
        guard let cli = Bundle.main.url(forResource: "iphone-call-export", withExtension: nil) else {
            presentError("Die eingebettete Export-Komponente fehlt. Bitte die App neu installieren.")
            return
        }

        isRunning = true
        status = "Export läuft …"
        log = ""

        let task = Process()
        task.executableURL = cli
        var args = [
            "--unlock",
            "--password-stdin",
            "--backup-root", backup.path.path,
            "--csv", outputURL.path
        ]
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
            Task { @MainActor in
                self?.log.append(text)
            }
        }

        task.terminationHandler = { [weak self] process in
            Task { @MainActor in
                output.fileHandleForReading.readabilityHandler = nil
                self?.isRunning = false
                self?.password = ""
                if process.terminationStatus == 0 {
                    self?.status = "Export abgeschlossen"
                    NSWorkspace.shared.activateFileViewerSelecting([self?.outputURL].compactMap { $0 })
                } else {
                    self?.status = "Export fehlgeschlagen"
                    self?.presentError("Der Export wurde mit Fehlercode \(process.terminationStatus) beendet. Details stehen im Protokollfenster.")
                }
                self?.process = nil
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

    private func scanBackups(in root: URL) -> [BackupItem] {
        if FileManager.default.fileExists(atPath: root.appendingPathComponent("Manifest.plist").path), let direct = readBackup(at: root) {
            return [direct]
        }
        guard let children = try? FileManager.default.contentsOfDirectory(
            at: root,
            includingPropertiesForKeys: [.contentModificationDateKey, .isDirectoryKey],
            options: [.skipsHiddenFiles]
        ) else { return [] }

        return children.compactMap(readBackup(at:)).sorted { $0.modified > $1.modified }
    }

    private func readBackup(at url: URL) -> BackupItem? {
        let manifestURL = url.appendingPathComponent("Manifest.plist")
        let infoURL = url.appendingPathComponent("Info.plist")
        guard FileManager.default.fileExists(atPath: manifestURL.path),
              let infoData = try? Data(contentsOf: infoURL),
              let info = try? PropertyListSerialization.propertyList(from: infoData, options: [], format: nil) as? [String: Any]
        else { return nil }

        var encrypted: Bool?
        if let manifestData = try? Data(contentsOf: manifestURL),
           let manifest = try? PropertyListSerialization.propertyList(from: manifestData, options: [], format: nil) as? [String: Any] {
            encrypted = manifest["IsEncrypted"] as? Bool
        }
        let values = try? url.resourceValues(forKeys: [.contentModificationDateKey])
        return BackupItem(
            path: url,
            deviceName: info["Device Name"] as? String ?? "",
            iosVersion: info["Product Version"] as? String ?? "",
            encrypted: encrypted,
            modified: values?.contentModificationDate ?? .distantPast
        )
    }

    private func presentError(_ message: String) {
        errorMessage = message
        showError = true
    }
}

struct ContentView: View {
    @StateObject private var model = AppModel()

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("iPhone Call Export")
                        .font(.largeTitle.bold())
                    Text("Anrufliste lokal aus einem Finder-Backup exportieren")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Backups neu laden") { model.refreshBackups() }
            }

            GroupBox("Backup") {
                VStack(alignment: .leading, spacing: 10) {
                    if model.backups.isEmpty {
                        Text("Keine Backups am Standardort gefunden.")
                            .foregroundStyle(.secondary)
                    } else {
                        Picker("Ausgewähltes Backup", selection: $model.selectedBackup) {
                            ForEach(model.backups) { backup in
                                VStack(alignment: .leading) {
                                    Text(backup.title)
                                    Text(backup.subtitle)
                                }
                                .tag(Optional(backup))
                            }
                        }
                        .pickerStyle(.menu)

                        if let selected = model.selectedBackup {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(selected.title).font(.headline)
                                Text(selected.subtitle).foregroundStyle(.secondary)
                                Text(selected.path.path)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)
                            }
                        }
                    }
                    Button("Anderen Backup-Ordner wählen …") { model.chooseBackupFolder() }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }

            GroupBox("Export") {
                VStack(alignment: .leading, spacing: 12) {
                    HStack {
                        TextField("CSV-Datei", text: Binding(
                            get: { model.outputURL.path },
                            set: { model.outputURL = URL(fileURLWithPath: $0) }
                        ))
                        Button("Auswählen …") { model.chooseOutput() }
                    }
                    Toggle("Kontakte aus dem iPhone-AddressBook abgleichen", isOn: $model.matchContacts)
                    SecureField("Backup-Passwort", text: $model.password)
                }
                .padding(4)
            }

            HStack {
                if model.isRunning { ProgressView().controlSize(.small) }
                Text(model.status).foregroundStyle(.secondary)
                Spacer()
                Button("Export starten") { model.startExport() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.isRunning || model.selectedBackup == nil)
            }

            DisclosureGroup("Protokoll") {
                ScrollView {
                    Text(model.log.isEmpty ? "Noch keine Ausgabe." : model.log)
                        .font(.caption.monospaced())
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                        .padding(8)
                }
                .frame(minHeight: 120, maxHeight: 220)
                .background(.quaternary.opacity(0.25))
                .clipShape(RoundedRectangle(cornerRadius: 8))
            }
        }
        .padding(24)
        .frame(minWidth: 720, minHeight: 560)
        .alert("iPhone Call Export", isPresented: $model.showError) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage)
        }
    }
}

@main
struct IPhoneCallExportApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
        .windowResizability(.contentSize)
    }
}
