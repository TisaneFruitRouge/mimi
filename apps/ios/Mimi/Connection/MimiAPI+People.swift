import Foundation

/// People: the directory, merging and splitting, removed contacts, and what's known about
/// someone (`/v1/people…`).
nonisolated extension MimiAPI {
    func people(_ query: String = "") async throws -> [PersonSummary] {
        try await call("GET", "/people?q=\(query.mimiQueryEscaped)")
    }

    func person(_ id: UUID) async throws -> Person { try await call("GET", "/people/\(id.mimiPath)") }

    func addPerson(_ p: NewPerson) async throws -> Person { try await call("POST", "/people", body: p) }

    func updatePerson(_ id: UUID, _ u: PersonUpdate) async throws -> Person {
        try await call("PATCH", "/people/\(id.mimiPath)", body: u)
    }

    /// Deletes someone from Mimi only. Says how to bring them back, or nil for someone the
    /// user had added by hand (nothing left to bring back).
    func removePerson(_ id: UUID) async throws -> RemovedPerson? {
        let data = try await raw("DELETE", "/people/\(id.mimiPath)")
        return try? JSONDecoder().decode(RemovedPerson.self, from: data)
    }

    func removedPeople() async throws -> [RemovedPerson] { try await call("GET", "/people/removed") }

    func restorePerson(_ id: UUID) async throws -> Person {
        try await call("POST", "/people/removed/\(id.mimiPath)/restore")
    }

    func addHandle(_ id: UUID, _ h: NewHandle) async throws -> Person {
        try await call("POST", "/people/\(id.mimiPath)/handles", body: h)
    }

    func updateHandle(_ id: UUID, handle: UUID, _ h: NewHandle) async throws -> Person {
        try await call("PATCH", "/people/\(id.mimiPath)/handles/\(handle.mimiPath)", body: h)
    }

    func removeHandle(_ id: UUID, handle: UUID) async throws -> Person {
        try await call("DELETE", "/people/\(id.mimiPath)/handles/\(handle.mimiPath)")
    }

    func mergePreview(_ r: MergeRequest) async throws -> MergePreview {
        try await call("POST", "/people/merge/preview", body: r)
    }

    func merge(_ r: MergeRequest) async throws -> MergeResult { try await call("POST", "/people/merge", body: r) }

    func undoMerge(_ mergeId: UUID) async throws -> Person {
        try await call("POST", "/people/merges/\(mergeId.mimiPath)/undo")
    }

    /// Separates a card: `sourceId` nil for what the user had added by hand.
    func splitPerson(_ id: UUID, sourceId: String?, record: String) async throws -> Person {
        struct Body: Encodable, Sendable {
            var source_id: String?
            var record: String
            func encode(to encoder: Encoder) throws {
                var c = encoder.container(keyedBy: CodingKeys.self)
                try c.encode(source_id, forKey: .source_id)
                try c.encode(record, forKey: .record)
            }
            enum CodingKeys: String, CodingKey { case source_id, record }
        }
        return try await call("POST", "/people/\(id.mimiPath)/split", body: Body(source_id: sourceId, record: record))
    }

    func duplicates() async throws -> [DuplicateSuggestion] { try await call("GET", "/people/duplicates") }

    func dismissDuplicate(_ a: UUID, _ b: UUID) async throws {
        _ = try await raw("POST", "/people/duplicates/dismiss", body: ["a": a.mimiPath, "b": b.mimiPath])
    }

    func syncPeople() async throws { _ = try await raw("POST", "/people/sync") }

    func personEvents(_ id: UUID, from: Int64, to: Int64) async throws -> [PersonEvent] {
        try await call("GET", "/people/\(id.mimiPath)/events?from=\(from)&to=\(to)")
    }

    func personConversations(_ id: UUID) async throws -> [PersonConversation] {
        try await call("GET", "/people/\(id.mimiPath)/conversations")
    }

    func personMemory(_ id: UUID) async throws -> [MemoryNote] {
        try await call("GET", "/people/\(id.mimiPath)/memory")
    }

    /// Recent email conversations with someone (none without a connected mailbox).
    func personMail(_ id: UUID) async throws -> [PersonMailThread] {
        try await call("GET", "/mail/threads?person=\(id.mimiPath)&limit=5")
    }
}

nonisolated extension UUID {
    /// How ids go into the computer's addresses.
    var mimiPath: String { uuidString.lowercased() }
}

nonisolated extension String {
    /// Escaped for a query value or a path segment: everything but letters, digits and
    /// `-._~` (like `encodeURIComponent`).
    var mimiQueryEscaped: String {
        var allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
        allowed.insert(charactersIn: "-._~")
        return addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
    }
}
