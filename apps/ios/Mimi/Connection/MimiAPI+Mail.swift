import Foundation

/// The `/v1/mail` calls (see `crates/core/src/api/mail.rs`).
nonisolated extension MimiAPI {
    /// A path with its query, each name and value percent-encoded (`+`, `&`, `=` included).
    static func path(_ base: String, _ items: [URLQueryItem]) -> String {
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~@")
        let query = items.compactMap { item -> String? in
            guard let value = item.value else { return nil }
            let n = item.name.addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
            let v = value.addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
            return "\(n)=\(v)"
        }
        return query.isEmpty ? base : "\(base)?\(query.joined(separator: "&"))"
    }

    /// Counts for `scope` (all mail when empty); accounts and folders are always all of them.
    func mailOverview(_ scope: MailScope = .all) async throws -> MailOverview {
        try await call("GET", Self.path("/mail", scope.queryItems))
    }

    /// Conversations in a view or a smart folder, or matching a search (which looks through
    /// everything). `before` pages back from the oldest one shown.
    func mailThreads(view: MailBox?, query: String = "", scope: MailScope = .all, folder: Int64? = nil,
                     before: Int64? = nil, limit: Int = 60) async throws -> [MailThread] {
        var items = scope.queryItems
        items.append(URLQueryItem(name: "limit", value: String(limit)))
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if !q.isEmpty {
            items.append(URLQueryItem(name: "q", value: q))
        } else if let folder {
            items.append(URLQueryItem(name: "folder", value: String(folder)))
        } else if let view {
            items.append(URLQueryItem(name: "view", value: view.rawValue))
        }
        if let before { items.append(URLQueryItem(name: "before", value: String(before))) }
        return try await call("GET", Self.path("/mail/threads", items))
    }

    func mailThread(_ id: Int64) async throws -> MailThreadDetail { try await call("GET", "/mail/threads/\(id)") }

    /// One email as sent (made safe) and as formatted text; pictures from other servers
    /// left out.
    func mailContent(_ message: Int64) async throws -> MailContent {
        try await call("GET", "/mail/messages/\(message)/content")
    }

    /// The same with its pictures from other servers, fetched by the computer now.
    func mailContentWithImages(_ message: Int64) async throws -> MailContent {
        try await call("POST", "/mail/messages/\(message)/images")
    }

    func markMailRead(_ id: Int64, read: Bool) async throws {
        struct Body: Encodable, Sendable { var read: Bool }
        _ = try await raw("POST", "/mail/threads/\(id)/read", body: Body(read: read))
    }

    func archiveMail(_ id: Int64) async throws { _ = try await raw("POST", "/mail/threads/\(id)/archive") }

    /// Moves a conversation to the Trash of its mail account.
    func deleteMail(_ id: Int64) async throws { _ = try await raw("DELETE", "/mail/threads/\(id)") }

    func summarizeMail(_ id: Int64) async throws -> String {
        let s: MailSummary = try await call("POST", "/mail/threads/\(id)/summarize")
        return s.summary
    }

    /// A reply written by the user's model, following their words if any.
    func draftMailReply(_ id: Int64, instructions: String?) async throws -> MailDraft {
        struct Body: Encodable, Sendable { var instructions: String? }
        return try await call("POST", "/mail/threads/\(id)/draft", body: Body(instructions: instructions))
    }

    /// Sends a message the user read and chose to send: their tap is the approval.
    func sendMail(_ draft: MailDraft) async throws { _ = try await raw("POST", "/mail/send", body: draft) }

    /// Asks every account to check for new mail now (it arrives as it's found).
    func refreshMail() async throws { _ = try await raw("POST", "/mail/refresh") }

    func createMailFolder(_ input: MailFolderInput) async throws { _ = try await raw("POST", "/mail/folders", body: input) }
    func updateMailFolder(_ id: Int64, _ input: MailFolderInput) async throws {
        _ = try await raw("PATCH", "/mail/folders/\(id)", body: input)
    }
    func deleteMailFolder(_ id: Int64) async throws { _ = try await raw("DELETE", "/mail/folders/\(id)") }

    /// Puts a conversation in a folder or takes it out; the sorter won't undo it.
    func setMailThreadFolder(_ thread: Int64, folder: Int64, member: Bool) async throws {
        struct Body: Encodable, Sendable { var folder: Int64; var member: Bool }
        _ = try await raw("POST", "/mail/threads/\(thread)/folders", body: Body(folder: folder, member: member))
    }

    /// The file itself, fetched from the mail server by the computer.
    func mailAttachment(message: Int64, index: Int) async throws -> Data {
        try await raw("GET", "/mail/messages/\(message)/attachments/\(index)")
    }

    /// Checks the TypeSafe key with TypeSafe, then saves it on the computer.
    func jevConnect(_ key: String) async throws {
        struct Body: Encodable, Sendable { var api_key: String }
        _ = try await raw("PUT", "/mail/jev", body: Body(api_key: key))
    }

    func jevDisconnect() async throws { _ = try await raw("DELETE", "/mail/jev") }

    /// The email accounts' states, to say when one can't be reached.
    func mailConnections() async throws -> [MailConnectionState] {
        let all: [MailConnectionState] = try await call("GET", "/connections")
        return all.filter { $0.integration == "email" }
    }
}
