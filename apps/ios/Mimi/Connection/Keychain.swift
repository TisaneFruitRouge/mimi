import Foundation
import Security

/// Small values kept in the Keychain, on this phone only (never synced to iCloud): the
/// phone's own key and what pairing gave it.
nonisolated enum Keychain {
    private static let service = "dev.mimi.ios"

    static func data(_ account: String) -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var out: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess else { return nil }
        return out as? Data
    }

    @discardableResult
    static func set(_ data: Data, for account: String) -> Bool {
        delete(account)
        let item: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: data,
            // Readable while the phone is locked (after the first unlock), never backed
            // up to another device.
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        return SecItemAdd(item as CFDictionary, nil) == errSecSuccess
    }

    static func delete(_ account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }

    static func value<T: Decodable>(_ type: T.Type, _ account: String) -> T? {
        data(account).flatMap { try? JSONDecoder().decode(T.self, from: $0) }
    }

    static func setValue<T: Encodable>(_ value: T, for account: String) {
        if let data = try? JSONEncoder().encode(value) { set(data, for: account) }
    }
}

/// What this phone needs to reach its computer, saved once pairing worked.
nonisolated struct PairedComputer: Codable, Sendable, Equatable {
    /// The computer's endpoint id (its public key).
    var endpointId: String
    /// The user's own relay, if the computer uses one. Nil: the public relays.
    var relay: String?
    /// This phone's token, sent with every request.
    var token: String
    var deviceId: UUID
    var deviceName: String

    private static let account = "paired-computer"

    static func load() -> PairedComputer? { Keychain.value(PairedComputer.self, account) }
    func save() { Keychain.setValue(self, for: Self.account) }
    static func forget() { Keychain.delete(account) }
}

/// The contents of a `mimi://pair?id=…&code=…[&relay=…]` link (the QR code).
nonisolated struct PairingLink: Equatable, Sendable {
    var endpointId: String
    var code: String
    var relay: String?

    init?(_ url: URL) {
        guard url.scheme == "mimi", url.host() == "pair",
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        else { return nil }
        func value(_ name: String) -> String? {
            items.first { $0.name == name }?.value.flatMap { $0.isEmpty ? nil : $0 }
        }
        guard let id = value("id"), let code = value("code"),
              id.count == 64, id.allSatisfy(\.isHexDigit)
        else { return nil }
        endpointId = id.lowercased()
        self.code = code
        relay = value("relay").flatMap { $0.hasPrefix("https://") || $0.hasPrefix("http://") ? $0 : nil }
    }

    init?(string: String) {
        guard let url = URL(string: string.trimmingCharacters(in: .whitespacesAndNewlines)) else { return nil }
        self.init(url)
    }
}
