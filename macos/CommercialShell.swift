import SwiftUI
import AppKit
import CryptoKit
import Security

private struct LicensePayload: Codable {
    let version: Int
    let licenseID: String
    let customer: String
    let issuedAt: String
    let expiresAt: String?
    let features: [String]
}

@MainActor
final class LicenseService: ObservableObject {
    static let shared = LicenseService()

    @Published private(set) var isLicensed = false
    @Published private(set) var customer = ""
    @Published private(set) var statusText = "Testversion · Exporte bis 100 Einträge"
    @Published var activationError = ""

    private let keychainService = "de.reuak.iphone-call-export"
    private let keychainAccount = "commercial-license-v1"

    private init() {
        if let stored = readKeychain() {
            _ = activate(stored)
        }
    }

    @discardableResult
    func activate(_ rawCode: String) -> Bool {
        activationError = ""
        let code = rawCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !code.isEmpty else {
            activationError = "Bitte einen Lizenzcode eingeben."
            return false
        }

        do {
            let payload = try verify(code)
            if let expires = payload.expiresAt {
                let formatter = ISO8601DateFormatter()
                guard let date = formatter.date(from: expires), date >= Date() else {
                    throw LicenseError.invalid("Die Lizenz ist abgelaufen.")
                }
            }
            guard payload.features.contains("unlimited-export") else {
                throw LicenseError.invalid("Diese Lizenz erlaubt keine unbegrenzten Exporte.")
            }

            try saveKeychain(code)
            isLicensed = true
            customer = payload.customer
            statusText = payload.customer.isEmpty ? "Vollversion aktiviert" : "Vollversion · \(payload.customer)"
            return true
        } catch {
            isLicensed = false
            customer = ""
            statusText = "Testversion · Exporte bis 100 Einträge"
            activationError = error.localizedDescription
            return false
        }
    }

    func deactivate() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: keychainAccount
        ]
        SecItemDelete(query as CFDictionary)
        isLicensed = false
        customer = ""
        statusText = "Testversion · Exporte bis 100 Einträge"
    }

    private func verify(_ code: String) throws -> LicensePayload {
        let parts = code.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 2,
              let payloadData = Data(base64URL: String(parts[0])),
              let signatureData = Data(base64URL: String(parts[1])) else {
            throw LicenseError.invalid("Der Lizenzcode hat ein ungültiges Format.")
        }
        guard let keyURL = Bundle.main.url(forResource: "license-public-key", withExtension: "der") else {
            throw LicenseError.invalid("Der öffentliche Lizenzschlüssel fehlt in der Installation.")
        }
        let publicKeyData = try Data(contentsOf: keyURL)
        let publicKey = try P256.Signing.PublicKey(derRepresentation: publicKeyData)
        let signature = try P256.Signing.ECDSASignature(derRepresentation: signatureData)
        guard publicKey.isValidSignature(signature, for: payloadData) else {
            throw LicenseError.invalid("Die Signatur des Lizenzcodes ist ungültig.")
        }
        let payload = try JSONDecoder().decode(LicensePayload.self, from: payloadData)
        guard payload.version == 1 else {
            throw LicenseError.invalid("Diese Lizenzversion wird nicht unterstützt.")
        }
        return payload
    }

    private func saveKeychain(_ code: String) throws {
        let data = Data(code.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: keychainAccount
        ]
        SecItemDelete(query as CFDictionary)
        var insert = query
        insert[kSecValueData as String] = data
        insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(insert as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw LicenseError.invalid("Die Lizenz konnte nicht im macOS-Schlüsselbund gespeichert werden (Fehler \(status)).")
        }
    }

    private func readKeychain() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: keychainAccount,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
}

private enum LicenseError: LocalizedError {
    case invalid(String)
    var errorDescription: String? {
        switch self { case .invalid(let message): return message }
    }
}

private extension Data {
    init?(base64URL: String) {
        var value = base64URL.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        let remainder = value.count % 4
        if remainder != 0 { value += String(repeating: "=", count: 4 - remainder) }
        self.init(base64Encoded: value)
    }
}

struct CommercialRootView: View {
    @AppStorage("onboardingCompletedV1") private var onboardingCompleted = false
    @StateObject private var license = LicenseService.shared
    @State private var showLicenseSheet = false

    var body: some View {
        if onboardingCompleted {
            VStack(spacing: 0) {
                HStack {
                    Label(license.statusText, systemImage: license.isLicensed ? "checkmark.seal.fill" : "lock.open")
                        .foregroundStyle(license.isLicensed ? .green : .secondary)
                    Spacer()
                    Button(license.isLicensed ? "Lizenz anzeigen" : "Lizenz aktivieren …") {
                        showLicenseSheet = true
                    }
                    Button("Anleitung") { onboardingCompleted = false }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(.bar)
                ContentView()
            }
            .sheet(isPresented: $showLicenseSheet) { LicenseView() }
        } else {
            WelcomeView(onStart: { onboardingCompleted = true }, onLicense: { showLicenseSheet = true })
                .sheet(isPresented: $showLicenseSheet) { LicenseView() }
        }
    }
}

private struct WelcomeView: View {
    let onStart: () -> Void
    let onLicense: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            VStack(alignment: .leading, spacing: 6) {
                Text("iPhone-Anrufliste exportieren")
                    .font(.largeTitle.bold())
                Text("Drei Schritte – die Daten bleiben auf diesem Mac.")
                    .font(.title3)
                    .foregroundStyle(.secondary)
            }

            HStack(alignment: .top, spacing: 22) {
                step(number: "1", icon: "cable.connector", title: "iPhone verbinden", text: "Verbinde das iPhone per Kabel mit dem Mac und bestätige bei Bedarf „Diesem Computer vertrauen“.")
                step(number: "2", icon: "externaldrive.badge.plus", title: "Backup erstellen", text: "Öffne das iPhone im Finder, aktiviere „Lokales Backup verschlüsseln“, vergib ein Kennwort und klicke auf „Backup jetzt erstellen“.")
                step(number: "3", icon: "tablecells", title: "Anrufe exportieren", text: "Die App findet das Backup, liest neue Telefonate ein und exportiert die gefilterte Ansicht als PDF, Excel oder CSV.")
            }

            GroupBox {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "info.circle.fill").foregroundStyle(.blue)
                    Text("Das Erstellen und erstmalige Verschlüsseln eines Finder-Backups kann nicht über eine von Apple dokumentierte öffentliche Schnittstelle vollständig im Hintergrund gesteuert werden. Die App öffnet deshalb den Finder-Ablauf und übernimmt anschließend Erkennung, Import und Export.")
                        .foregroundStyle(.secondary)
                }
                .padding(4)
            }

            HStack {
                Button("Lizenz aktivieren …", action: onLicense)
                Spacer()
                Button("Start", action: onStart)
                    .keyboardShortcut(.defaultAction)
                    .controlSize(.large)
            }
        }
        .padding(36)
        .frame(minWidth: 920, minHeight: 560)
    }

    private func step(number: String, icon: String, title: String, text: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(number)
                    .font(.headline)
                    .frame(width: 30, height: 30)
                    .background(Circle().fill(.tint))
                    .foregroundStyle(.white)
                Image(systemName: icon).font(.title2)
            }
            Text(title).font(.title3.bold())
            Text(text).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(18)
        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 14))
    }
}

private struct LicenseView: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject private var license = LicenseService.shared
    @State private var code = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Lizenz").font(.title.bold())
            Text(license.isLicensed ? license.statusText : "Ohne Lizenz können höchstens 100 gefilterte Einträge pro Export ausgegeben werden.")
                .foregroundStyle(.secondary)

            if !license.isLicensed {
                TextEditor(text: $code)
                    .font(.body.monospaced())
                    .frame(minHeight: 110)
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(.separator))
                if !license.activationError.isEmpty {
                    Text(license.activationError).foregroundStyle(.red)
                }
                HStack {
                    Spacer()
                    Button("Aktivieren") {
                        if license.activate(code) { dismiss() }
                    }
                    .keyboardShortcut(.defaultAction)
                }
            } else {
                HStack {
                    Button("Lizenz auf diesem Mac entfernen", role: .destructive) { license.deactivate() }
                    Spacer()
                    Button("Schließen") { dismiss() }.keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(24)
        .frame(width: 580)
    }
}

@main
struct IPhoneCallExportCommercialApp: App {
    var body: some Scene {
        WindowGroup { CommercialRootView() }
            .windowStyle(.titleBar)
    }
}
