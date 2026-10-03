import Foundation

nonisolated enum MimiError: LocalizedError, Equatable {
    /// The computer couldn't be reached (asleep, offline, Mimi not running).
    case unreachable
    /// The computer doesn't know this phone any more (it was removed).
    case unpaired
    /// The computer said no, with a message meant for the user.
    case api(code: String, message: String)
    /// A route the computer doesn't have: its Mimi is older than this app.
    case outdated
    case http(Int)
    case unreadable

    var errorDescription: String? {
        switch self {
        case .unreachable: "Your computer can't be reached right now. Check that it's on and connected."
        case .unpaired: "This phone was removed from your computer. Pair it again to keep using Mimi."
        case .api(_, let message): message
        case .outdated: "Mimi on your computer needs an update for this."
        case .http(let status): "Something went wrong on your computer (\(status))."
        case .unreadable: "Your computer sent something this app couldn't read. Is Mimi up to date on both?"
        }
    }
}

/// Typed calls to the computer's `/v1` API, over the phone's link.
nonisolated struct MimiAPI: Sendable {
    let link: DaemonLink

    // MARK: Plumbing

    func call<T: Decodable & Sendable>(_ method: String, _ path: String, body: (any Encodable & Sendable)? = nil, as: T.Type = T.self) async throws -> T {
        let data = try await raw(method, path, body: body)
        if T.self == Empty.self { return Empty() as! T }
        do {
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            throw MimiError.unreadable
        }
    }

    func raw(_ method: String, _ path: String, body: (any Encodable & Sendable)? = nil) async throws -> Data {
        let encoded = try body.map { try JSONEncoder().encode($0) }
        let res = try await link.send(method, "/v1" + path, body: encoded)
        switch res.status {
        case 200..<300: return res.body
        case 401: throw MimiError.unpaired
        default:
            if let err = try? JSONDecoder().decode(ApiErrorBody.self, from: res.body) {
                if err.code == "not_found", err.message == "Route not found" { throw MimiError.outdated }
                throw MimiError.api(code: err.code, message: err.message)
            }
            throw res.status == 404 || res.status == 405 ? MimiError.outdated : MimiError.http(res.status)
        }
    }

    nonisolated struct Empty: Codable, Sendable {}

    // MARK: Pairing and this phone

    func pair(code: String, name: String) async throws -> Paired {
        try await call("POST", "/remote/pair", body: PairRequest(code: code, name: name))
    }

    func remote() async throws -> RemoteAccess { try await call("GET", "/remote") }

    func removeThisPhone(_ id: UUID) async throws {
        _ = try await raw("DELETE", "/remote/devices/\(id.uuidString.lowercased())")
    }

    func renamePhone(_ id: UUID, name: String) async throws {
        struct Body: Encodable, Sendable { var name: String }
        _ = try await raw("PATCH", "/remote/devices/\(id.uuidString.lowercased())", body: Body(name: name))
    }

    // MARK: Settings and models

    func settingsJSON() async throws -> JSONValue { try await call("GET", "/settings") }
    func putSettings(_ settings: JSONValue) async throws -> JSONValue { try await call("PUT", "/settings", body: settings) }
    func providers() async throws -> [Provider] { try await call("GET", "/providers") }

    // MARK: Chat

    func conversations() async throws -> [Conversation] { try await call("GET", "/conversations") }

    func conversation(_ id: UUID) async throws -> ConversationDetail {
        try await call("GET", "/conversations/\(id.uuidString.lowercased())")
    }

    func createConversation() async throws -> Conversation {
        try await call("POST", "/conversations", body: [String: String]())
    }

    func renameConversation(_ id: UUID, title: String) async throws -> Conversation {
        try await call("PATCH", "/conversations/\(id.uuidString.lowercased())", body: ["title": title])
    }

    func deleteConversation(_ id: UUID) async throws {
        _ = try await raw("DELETE", "/conversations/\(id.uuidString.lowercased())")
    }

    func send(_ id: UUID, _ message: SendMessage) async throws -> SendMessageResult {
        try await call("POST", "/conversations/\(id.uuidString.lowercased())/messages", body: message)
    }

    func cancel(_ id: UUID) async throws {
        _ = try await raw("POST", "/conversations/\(id.uuidString.lowercased())/cancel")
    }

    func approve(_ action: UUID, always: Bool = false) async throws {
        struct Body: Encodable, Sendable { var arguments: JSONValue? = nil; var always: Bool }
        _ = try await raw("POST", "/actions/\(action.uuidString.lowercased())/approve", body: Body(always: always))
    }

    func reject(_ action: UUID) async throws {
        _ = try await raw("POST", "/actions/\(action.uuidString.lowercased())/reject")
    }

    func undoMemory(_ revision: Int) async throws { _ = try await raw("POST", "/memory/undo/\(revision)") }
    func undoSchedule(_ revision: Int) async throws { _ = try await raw("POST", "/schedule/undo/\(revision)") }

    /// @ suggestions (people and events), or # suggestions (email).
    func mentions(_ query: String, mail: Bool) async throws -> [MentionCandidate] {
        let q = query.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed.subtracting(CharacterSet(charactersIn: "&=+#"))) ?? ""
        return try await call("GET", "/mentions?q=\(q)\(mail ? "&kind=mail" : "")")
    }

    // MARK: Events

    /// The computer's event feed, one event at a time. Ends when the connection does.
    func events() async throws -> AsyncThrowingStream<Event?, Error> {
        let lines = try await link.lines("/v1/events", headers: ["accept": "application/x-ndjson"])
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    for try await line in lines {
                        if line.isEmpty {
                            // Heartbeat.
                            continuation.yield(nil)
                            continue
                        }
                        if let event = try? JSONDecoder().decode(Event.self, from: Data(line.utf8)) {
                            continuation.yield(event)
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
}
