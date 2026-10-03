import Foundation

/// What the Settings pages call: memory, permissions, model sources, connections,
/// updates and the computer's hardware.
nonisolated extension MimiAPI {
    // MARK: Memory

    func memory() async throws -> MemoryOverview { try await call("GET", "/memory") }

    func memoryNote(_ path: String) async throws -> MemoryNote {
        try await call("GET", "/memory/note?path=\(path.mimiQueryEscaped)")
    }

    @discardableResult
    func saveMemoryNote(_ path: String, title: String?, body: String) async throws -> MemoryNote {
        struct Edit: Encodable, Sendable {
            var title: String?
            var body: String
            func encode(to encoder: Encoder) throws {
                var c = encoder.container(keyedBy: CodingKeys.self)
                try c.encode(title, forKey: .title)
                try c.encode(body, forKey: .body)
            }
            enum CodingKeys: String, CodingKey { case title, body }
        }
        return try await call("PUT", "/memory/note?path=\(path.mimiQueryEscaped)", body: Edit(title: title, body: body))
    }

    func deleteMemoryNote(_ path: String) async throws {
        _ = try await raw("DELETE", "/memory/note?path=\(path.mimiQueryEscaped)")
    }

    func saveMemoryProfile(_ text: String) async throws {
        _ = try await raw("PUT", "/memory/profile", body: ["text": text])
    }

    func setMemoryLearning(_ on: Bool) async throws {
        _ = try await raw("PUT", "/memory/learning", body: ["learning": on])
    }

    func setMemorySemantic(_ on: Bool) async throws -> MemorySemantic {
        try await call("PUT", "/memory/semantic", body: ["enabled": on])
    }

    func forgetEverything() async throws { _ = try await raw("POST", "/memory/forget-all") }

    // MARK: Permissions

    func permissions() async throws -> [PermissionKind] { try await call("GET", "/permissions") }

    func setPermission(_ kind: String, _ choice: KindPermission) async throws -> [PermissionKind] {
        try await call("PUT", "/permissions/\(kind.mimiQueryEscaped)", body: choice)
    }

    func calendarsForPermissions() async throws -> [PermissionCalendar] { try await call("GET", "/calendars") }

    // MARK: Models

    func modelSources() async throws -> [ModelSource] { try await call("GET", "/providers") }
    func presets() async throws -> [ProviderPreset] { try await call("GET", "/providers/presets") }
    func probe(_ r: ProbeRequest) async throws -> ProbeResult { try await call("POST", "/providers/probe", body: r) }
    func addProvider(_ p: NewProvider) async throws -> ModelSource { try await call("POST", "/providers", body: p) }

    func updateProvider(_ id: UUID, _ u: ProviderUpdate) async throws -> ModelSource {
        try await call("PATCH", "/providers/\(id.mimiPath)", body: u)
    }

    func removeProvider(_ id: UUID) async throws { _ = try await raw("DELETE", "/providers/\(id.mimiPath)") }

    func models(_ providerId: UUID) async throws -> [ModelInfo] {
        try await call("GET", "/providers/\(providerId.mimiPath)/models")
    }

    func pull(_ providerId: UUID, model: String) async throws -> ModelPull {
        try await call("POST", "/providers/\(providerId.mimiPath)/pull", body: ["model": model])
    }

    func cancelPull(_ providerId: UUID, model: String) async throws {
        _ = try await raw("POST", "/providers/\(providerId.mimiPath)/pull/cancel", body: ["model": model])
    }

    func deleteModel(_ providerId: UUID, model: String) async throws {
        _ = try await raw("DELETE", "/providers/\(providerId.mimiPath)/models/\(model.mimiQueryEscaped)")
    }

    func pulls() async throws -> [ModelPull] { try await call("GET", "/pulls") }
    func runtime() async throws -> RuntimeStatus { try await call("GET", "/runtime") }
    func catalog() async throws -> [CatalogModel] { try await call("GET", "/catalog") }
    func recommendations() async throws -> Recommendations { try await call("GET", "/recommendations") }

    // MARK: Connections

    func integrations() async throws -> [Integration] { try await call("GET", "/integrations") }
    func connections() async throws -> [ConnectionItem] { try await call("GET", "/connections") }

    func connect(_ setup: ConnectionSetup) async throws -> ConnectionItem {
        try await call("POST", "/connections", body: setup)
    }

    func disconnect(_ id: UUID) async throws { _ = try await raw("DELETE", "/connections/\(id.mimiPath)") }

    func googleSignInInfo() async throws -> GoogleSignInInfo { try await call("GET", "/google/sign-in") }

    func discoverMail(_ email: String) async throws -> MailServerDiscovery {
        try await call("POST", "/mail/discover", body: ["email": email])
    }

    // MARK: Updates

    func updates() async throws -> UpdateStatus { try await call("GET", "/updates") }
    func checkForUpdates() async throws -> UpdateStatus { try await call("POST", "/updates/check") }
}
