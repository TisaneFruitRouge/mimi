import Foundation

// Mirrors of what the Settings pages read: memory.rs, permissions.rs, providers.rs,
// connections.rs, integrations.rs, updates.rs, hardware.rs, and the mail discovery used to
// connect an email account. Only what the app reads is declared.

// MARK: Memory

nonisolated enum MemorySource: String, Codable, Sendable {
    case you, assistant, learned

    init(from decoder: Decoder) throws {
        self = MemorySource(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .assistant
    }

    var label: String {
        switch self {
        case .you: "Written by you"
        case .assistant: "Noted during a conversation"
        case .learned: "Learned from your conversations"
        }
    }
}

nonisolated struct MemoryNote: Decodable, Sendable, Identifiable, Hashable {
    var path: String
    var title: String
    var body: String
    var subject: String?
    var subjectName: String?
    var source: MemorySource
    var updatedAt: Int64
    var id: String { path }

    enum CodingKeys: String, CodingKey {
        case path, title, body, subject, source
        case subjectName = "subject_name"
        case updatedAt = "updated_at"
    }
}

nonisolated struct MemoryNoteSummary: Decodable, Sendable, Identifiable, Hashable {
    var path: String
    var title: String
    var preview: String
    var subject: String?
    var subjectName: String?
    var source: MemorySource
    var updatedAt: Int64
    var id: String { path }

    enum CodingKeys: String, CodingKey {
        case path, title, preview, subject, source
        case subjectName = "subject_name"
        case updatedAt = "updated_at"
    }
}

nonisolated struct MemorySemantic: Decodable, Sendable, Equatable {
    var enabled: Bool
    var available: Bool
    var installed: Bool
    var downloadBytes: Int64
    var providerId: UUID?
    var model: String
    var indexed: Int
    var total: Int
    var error: String?

    enum CodingKeys: String, CodingKey {
        case enabled, available, installed, model, indexed, total, error
        case downloadBytes = "download_bytes"
        case providerId = "provider_id"
    }
}

nonisolated struct MemoryOverview: Decodable, Sendable {
    var profile: String
    var profileLimit: Int
    var notes: [MemoryNoteSummary]
    var learning: Bool
    var semantic: MemorySemantic?

    enum CodingKeys: String, CodingKey {
        case profile, notes, learning, semantic
        case profileLimit = "profile_limit"
    }
}

/// The library's folders, as the user sees them. Mirrors `memory::FOLDERS` in the daemon.
nonisolated enum MemoryFolders {
    static let all: [(id: String, label: String)] = [
        ("people", "People"),
        ("habits", "Habits and routines"),
        ("preferences", "Likes and preferences"),
        ("places", "Places"),
        ("work", "Work"),
        ("interests", "Interests"),
        ("health", "Health"),
        ("notes", "Other"),
    ]

    static func folder(of path: String) -> String {
        let f = path.split(separator: "/").first.map(String.init) ?? "notes"
        return all.contains { $0.id == f } ? f : "notes"
    }

    /// Notes grouped by folder, in the folders' order, empty ones left out.
    static func groups(_ notes: [MemoryNoteSummary]) -> [(id: String, label: String, notes: [MemoryNoteSummary])] {
        all.compactMap { f in
            let inside = notes.filter { folder(of: $0.path) == f.id }
            return inside.isEmpty ? nil : (f.id, f.label, inside)
        }
    }

    /// Notes speak of "the user"; previews read better addressed to the reader.
    static func forYou(_ text: String) -> String {
        var t = text
        for (from, to) in [
            ("The user's", "Your"), ("the user's", "your"),
            ("The user is", "You are"), ("the user is", "you are"),
            ("The user", "You"), ("the user", "you"),
        ] {
            t = t.replacingOccurrences(of: "\\b\(from)\\b", with: to, options: .regularExpression)
        }
        return t
    }
}

// MARK: Permissions

nonisolated enum Autonomy: String, Codable, Sendable, CaseIterable {
    case ask, automatic

    var label: String { self == .ask ? "Ask first" : "Automatic" }
}

/// What an exception is about: a person (by id) or a calendar (by its stable id).
nonisolated struct PermissionTarget: Codable, Sendable, Hashable {
    var kind: String
    var id: String

    var isPerson: Bool { kind == "person" }
}

nonisolated struct PermissionRuleView: Codable, Sendable, Hashable {
    var target: PermissionTarget
    var autonomy: Autonomy
    var label: String
    var missing: Bool

    enum CodingKeys: String, CodingKey { case target, autonomy, label, missing }

    init(target: PermissionTarget, autonomy: Autonomy, label: String, missing: Bool = false) {
        self.target = target
        self.autonomy = autonomy
        self.label = label
        self.missing = missing
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        target = try c.decode(PermissionTarget.self, forKey: .target)
        autonomy = try c.decode(Autonomy.self, forKey: .autonomy)
        label = try c.decodeIfPresent(String.self, forKey: .label) ?? ""
        missing = try c.decodeIfPresent(Bool.self, forKey: .missing) ?? false
    }
}

/// A kind of action, as `GET /v1/permissions` serves it.
nonisolated struct PermissionKind: Decodable, Sendable, Identifiable, Hashable {
    var id: String
    var title: String
    var askDetail: String
    var automaticDetail: String
    var note: String?
    var icon: String
    var color: String
    /// "none", "person" or "calendar".
    var targets: String
    var autonomy: Autonomy
    var rules: [PermissionRuleView]

    enum CodingKeys: String, CodingKey {
        case id, title, note, icon, color, targets, autonomy, rules
        case askDetail = "ask_detail"
        case automaticDetail = "automatic_detail"
    }

    var detail: String { autonomy == .ask ? askDetail : automaticDetail }
}

/// `PUT /v1/permissions/{kind}`.
nonisolated struct KindPermission: Encodable, Sendable {
    struct Rule: Encodable, Sendable {
        var target: PermissionTarget
        var autonomy: Autonomy
    }

    var autonomy: Autonomy
    var rules: [Rule]
}

/// A calendar an exception can be about (`GET /v1/calendars`).
nonisolated struct PermissionCalendar: Decodable, Sendable, Identifiable, Hashable {
    var id: String
    var name: String
    var color: String?
}

// MARK: Models

nonisolated enum ProviderKind: String, Codable, Sendable {
    case openaiCompatible = "openai_compatible"
    case builtin
    case anthropic

    init(from decoder: Decoder) throws {
        self = ProviderKind(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .openaiCompatible
    }
}

/// A model source with everything the Models page shows (the app's `Provider` only keeps
/// what chat needs).
nonisolated struct ModelSource: Decodable, Sendable, Identifiable, Hashable {
    var id: UUID
    var name: String
    var kind: ProviderKind
    var baseUrl: String
    var locality: Locality
    var hasApiKey: Bool

    enum CodingKeys: String, CodingKey {
        case id, name, kind, locality
        case baseUrl = "base_url"
        case hasApiKey = "has_api_key"
    }
}

nonisolated struct ProviderPreset: Decodable, Sendable, Identifiable, Hashable {
    var id: String
    var name: String
    var description: String
    var kind: ProviderKind
    var baseUrl: String
    var locality: Locality
    var needsApiKey: Bool
    var keyUrl: String?
    var recommendedModel: String?

    enum CodingKeys: String, CodingKey {
        case id, name, description, kind, locality
        case baseUrl = "base_url"
        case needsApiKey = "needs_api_key"
        case keyUrl = "key_url"
        case recommendedModel = "recommended_model"
    }
}

nonisolated struct ModelPrice: Decodable, Sendable, Hashable {
    var input: Double
    var output: Double
}

nonisolated struct ModelInfo: Decodable, Sendable, Hashable {
    var id: String
    var name: String?
    var sizeBytes: Int64?
    var supportsTools: Bool?
    var price: ModelPrice?

    enum CodingKeys: String, CodingKey {
        case id, name, price
        case sizeBytes = "size_bytes"
        case supportsTools = "supports_tools"
    }
}

nonisolated struct ProbeRequest: Encodable, Sendable {
    var kind: ProviderKind
    var baseUrl: String
    var apiKey: String?

    enum CodingKeys: String, CodingKey {
        case kind
        case baseUrl = "base_url"
        case apiKey = "api_key"
    }
}

nonisolated struct ProbeResult: Decodable, Sendable, Hashable {
    var baseUrl: String
    var locality: Locality
    var models: [ModelInfo]
    enum CodingKeys: String, CodingKey { case locality, models, baseUrl = "base_url" }
}

nonisolated struct NewProvider: Encodable, Sendable {
    var name: String
    var kind: ProviderKind
    var baseUrl: String
    var apiKey: String?

    enum CodingKeys: String, CodingKey {
        case name, kind
        case baseUrl = "base_url"
        case apiKey = "api_key"
    }
}

/// Fields left nil are unchanged.
nonisolated struct ProviderUpdate: Encodable, Sendable {
    var name: String?
    var baseUrl: String?
    var apiKey: String?
    var locality: Locality?

    enum CodingKeys: String, CodingKey {
        case name, locality
        case baseUrl = "base_url"
        case apiKey = "api_key"
    }
}

nonisolated enum PullState: String, Decodable, Sendable {
    case running, done, failed, cancelled
}

/// A model download on the computer.
nonisolated struct ModelPull: Decodable, Sendable, Hashable {
    var providerId: UUID
    var model: String
    var state: PullState
    var status: String
    var completedBytes: Int64?
    var totalBytes: Int64?
    var error: String?

    enum CodingKeys: String, CodingKey {
        case model, state, status, error
        case providerId = "provider_id"
        case completedBytes = "completed_bytes"
        case totalBytes = "total_bytes"
    }

    var fraction: Double? {
        guard let total = totalBytes, total > 0, let done = completedBytes else { return nil }
        return min(1, Double(done) / Double(total))
    }
}

nonisolated struct RuntimeStatus: Decodable, Sendable, Hashable {
    var available: Bool
    var state: String
    var model: String?
    var error: String?
    var modelsBytes: Int64
    enum CodingKeys: String, CodingKey { case available, state, model, error, modelsBytes = "models_bytes" }
}

nonisolated struct CatalogModel: Decodable, Sendable, Identifiable, Hashable {
    var id: String
    var name: String
    var description: String
    var downloadBytes: Int64
    enum CodingKeys: String, CodingKey { case id, name, description, downloadBytes = "download_bytes" }
}

nonisolated struct GpuInfo: Decodable, Sendable, Hashable {
    var name: String
    var kind: String
}

nonisolated struct HardwareInfo: Decodable, Sendable, Hashable {
    var totalMemoryBytes: Int64
    var gpus: [GpuInfo]
    enum CodingKeys: String, CodingKey { case gpus, totalMemoryBytes = "total_memory_bytes" }
}

nonisolated struct InstalledModel: Decodable, Sendable, Hashable {
    var model: ModelRef
    var fits: Bool
}

nonisolated struct DetectedServer: Decodable, Sendable, Hashable, Identifiable {
    var presetId: String
    var name: String
    var baseUrl: String
    var modelCount: Int
    var alreadyAdded: Bool
    var id: String { presetId }

    enum CodingKeys: String, CodingKey {
        case name
        case presetId = "preset_id"
        case baseUrl = "base_url"
        case modelCount = "model_count"
        case alreadyAdded = "already_added"
    }
}

nonisolated struct Recommendations: Decodable, Sendable {
    var hardware: HardwareInfo
    var tier: String
    var modelBudgetBytes: Int64
    var preferCloud: Bool
    var installed: [InstalledModel]
    var suggested: [CatalogModel]
    var detectedServers: [DetectedServer]
    var downloadProviderId: UUID?

    enum CodingKeys: String, CodingKey {
        case hardware, tier, installed, suggested
        case modelBudgetBytes = "model_budget_bytes"
        case preferCloud = "prefer_cloud"
        case detectedServers = "detected_servers"
        case downloadProviderId = "download_provider_id"
    }

    var tierLine: String {
        switch tier {
        case "minimal": "Best with cloud models. Local ones will be slow there."
        case "light": "Runs small models well."
        case "standard": "Runs mid-sized models comfortably."
        case "strong": "Runs large models comfortably."
        default: "Runs very large models."
        }
    }
}

/// One model from one source, as the lists show it.
nonisolated struct ModelOption: Identifiable, Hashable, Sendable {
    var ref: ModelRef
    var name: String
    var maker: String?
    var description: String?
    var sourceName: String
    var locality: Locality
    var sizeBytes: Int64?
    var supportsTools: Bool?
    var price: ModelPrice?
    var id: String { "\(ref.providerId)/\(ref.model)" }
}

nonisolated enum ModelNames {
    /// Friendly name, maker and description for a model id: from the catalog when it's
    /// known, else the name its source gave it, else a tidied id (like `useModelInfo`).
    static func info(_ id: String, sourceName: String?, catalog: [CatalogModel]) -> (name: String, maker: String?, description: String?) {
        let bare = id.hasSuffix(":latest") ? String(id.dropLast(7)) : id
        let known = catalog.first { $0.id == id || $0.id == bare }
        var maker: String?
        var name = sourceName
        // OpenRouter names say who made the model: "DeepSeek: DeepSeek V4 Flash".
        if let s = sourceName, let r = s.range(of: ": ") {
            maker = String(s[..<r.lowerBound])
            name = String(s[r.upperBound...])
        }
        return (known?.name ?? name ?? prettify(id), maker, known?.description)
    }

    static func prettify(_ id: String) -> String {
        let last = id.split(separator: "/").last.map(String.init) ?? id
        let parts = last.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2, parts[1] != "latest" else { return parts.first ?? id }
        return "\(parts[0]) \(parts[1].uppercased())"
    }

    /// Roughly what one message costs: Mimi sends about 6,000 tokens and gets a few
    /// hundred back.
    static func costLabel(_ price: ModelPrice) -> String {
        if price.input == 0 && price.output == 0 { return "Free" }
        let cents = (6000 * price.input + 300 * price.output) / 1e6 * 100
        if cents < 0.01 { return "under 0.01¢ a message" }
        if cents < 0.1 { return String(format: "about %.2f¢ a message", cents) }
        if cents < 100 { return String(format: "about %.1f¢ a message", cents) }
        return String(format: "about $%.2f a message", cents / 100)
    }

    static func centsPerMessage(_ price: ModelPrice?) -> Double {
        guard let price else { return .infinity }
        return (6000 * price.input + 300 * price.output) / 1e6 * 100
    }
}

/// "3.3 GB", for downloads and sizes.
nonisolated func formatBytes(_ bytes: Int64) -> String {
    ByteCountFormatter.string(fromByteCount: bytes, countStyle: .decimal)
}

// MARK: Connections

nonisolated enum ConnectionStatus: String, Decodable, Sendable {
    case ok
    case needsAction = "needs_action"
    case error

    init(from decoder: Decoder) throws {
        self = ConnectionStatus(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .error
    }
}

/// One of the user's connected accounts (a `Connection`).
nonisolated struct ConnectionItem: Decodable, Sendable, Identifiable, Hashable {
    var id: UUID
    var integration: String
    var name: String
    var status: ConnectionStatus
    var detail: String
    var actionUrl: String?

    enum CodingKeys: String, CodingKey {
        case id, integration, name, status, detail
        case actionUrl = "action_url"
    }
}

nonisolated struct Integration: Decodable, Sendable, Identifiable, Hashable {
    var id: String
    var name: String
    var category: String
    var description: String
    var abilities: [String]
    /// "connected", "available" or "coming_soon".
    var status: String
}

/// What the user entered to connect something (`ConnectionSetup`).
nonisolated enum ConnectionSetup: Encodable, Sendable {
    case googleCalendar(icsUrl: String)
    case caldav(serverUrl: String, username: String, password: String)
    case telegram(botToken: String)
    case email(email: String, password: String, preset: String?, servers: MailServerSettings?)

    private enum Keys: String, CodingKey {
        case integration
        case icsUrl = "ics_url"
        case serverUrl = "server_url"
        case username, password, email, preset, servers
        case botToken = "bot_token"
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .googleCalendar(let url):
            try c.encode("google_calendar", forKey: .integration)
            try c.encode(url, forKey: .icsUrl)
        case .caldav(let server, let user, let password):
            try c.encode("caldav", forKey: .integration)
            try c.encode(server, forKey: .serverUrl)
            try c.encode(user, forKey: .username)
            try c.encode(password, forKey: .password)
        case .telegram(let token):
            try c.encode("telegram", forKey: .integration)
            try c.encode(token, forKey: .botToken)
        case .email(let email, let password, let preset, let servers):
            try c.encode("email", forKey: .integration)
            try c.encode(email, forKey: .email)
            try c.encode(password, forKey: .password)
            try c.encode(preset, forKey: .preset)
            try c.encode(servers, forKey: .servers)
        }
    }
}

nonisolated enum MailSecuritySetting: String, Codable, Sendable, CaseIterable {
    case tls
    case startTls = "start_tls"
    case plain

    var label: String {
        switch self {
        case .tls: "SSL/TLS"
        case .startTls: "STARTTLS"
        case .plain: "None (computer only)"
        }
    }
}

/// Where an account's mail lives (`MailServers`).
nonisolated struct MailServerSettings: Codable, Sendable, Equatable {
    var imapHost = ""
    var imapPort = 993
    var imapSecurity = MailSecuritySetting.tls
    var smtpHost = ""
    var smtpPort = 465
    var smtpSecurity = MailSecuritySetting.tls
    var username: String?

    enum CodingKeys: String, CodingKey {
        case username
        case imapHost = "imap_host"
        case imapPort = "imap_port"
        case imapSecurity = "imap_security"
        case smtpHost = "smtp_host"
        case smtpPort = "smtp_port"
        case smtpSecurity = "smtp_security"
    }
}

/// What the computer worked out from an email address (`MailDiscovery`).
nonisolated struct MailServerDiscovery: Decodable, Sendable, Equatable {
    var provider: String?
    var preset: String?
    var servers: MailServerSettings?
    var needsAppPassword: Bool
    var help: String?
    var helpUrl: String?
    var supported: Bool

    enum CodingKeys: String, CodingKey {
        case provider, preset, servers, help, supported
        case needsAppPassword = "needs_app_password"
        case helpUrl = "help_url"
    }
}

nonisolated struct GoogleSignInInfo: Decodable, Sendable {
    var available: Bool
}

// MARK: Updates

nonisolated struct Release: Decodable, Sendable, Hashable {
    var version: String
    var url: String
}

nonisolated struct UpdateStatus: Decodable, Sendable, Hashable {
    var current: String
    var available: Release?
    var checkedAt: Int64?
    var error: String?
    enum CodingKeys: String, CodingKey { case current, available, error, checkedAt = "checked_at" }
}

/// "just now", "5 min ago", "2 hours ago", "yesterday", "3 days ago".
nonisolated func timeSince(_ ms: Int64, now: Date = .now) -> String {
    let minutes = Int((now.timeIntervalSince1970 * 1000 - Double(ms)) / 60_000)
    if minutes < 1 { return "just now" }
    if minutes < 60 { return "\(minutes) min ago" }
    let hours = Int((Double(minutes) / 60).rounded())
    if hours < 24 { return hours == 1 ? "an hour ago" : "\(hours) hours ago" }
    let days = Int((Double(hours) / 24).rounded())
    return days == 1 ? "yesterday" : "\(days) days ago"
}
