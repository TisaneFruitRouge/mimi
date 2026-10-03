import Foundation

// Mirrors of `crates/protocol/src/mail.rs` (and `MailSorter` from settings.rs). Only what
// the app reads is declared; unknown fields are ignored.

/// What part of the mail to show.
nonisolated enum MailBox: String, Codable, Sendable, CaseIterable, Identifiable {
    case needsReply = "needs_reply"
    case important, other, inbox, sent, archive

    var id: String { rawValue }

    /// Views the sorter fills (shown only while sorting is on).
    var sorted: Bool { self == .needsReply || self == .important || self == .other }

    var label: String {
        switch self {
        case .needsReply: "Needs a reply"
        case .important: "Important"
        case .other: "Everything else"
        case .inbox: "Inbox"
        case .sent: "Sent"
        case .archive: "Archive"
        }
    }

    var symbol: String {
        switch self {
        case .needsReply: "arrowshape.turn.up.left"
        case .important: "star"
        case .other: "square.stack"
        case .inbox: "tray"
        case .sent: "paperplane"
        case .archive: "archivebox"
        }
    }

    var empty: String {
        switch self {
        case .needsReply: "Nothing is waiting for your answer."
        case .important: "Nothing important right now."
        case .other: "No newsletters or notifications."
        case .inbox: "Your inbox is empty."
        case .sent: "Nothing sent in the last 90 days."
        case .archive: "Nothing archived in the last 90 days."
        }
    }
}

nonisolated enum MailCategory: String, Codable, Sendable {
    case needsReply = "needs_reply"
    case important, other
}

nonisolated enum MailSorter: String, Codable, Sendable {
    case model, jev
}

nonisolated struct MailAddress: Codable, Sendable, Hashable {
    var name: String?
    var email: String

    /// The name, else the part of the address before the @.
    var shortName: String {
        if let name, !name.trimmingCharacters(in: .whitespaces).isEmpty { return name }
        return String(email.split(separator: "@").first ?? Substring(email))
    }

    /// "Sam Carter <sam@example.com>", or just the address.
    var full: String { name.map { "\($0) <\(email)>" } ?? email }
}

/// One conversation, as the list shows it.
nonisolated struct MailThread: Codable, Sendable, Identifiable, Hashable {
    var id: Int64
    var connectionId: UUID
    var receivedOn: String?
    var suspicious: Bool
    var folders: [Int64]
    var subject: String
    var participants: [MailAddress]
    var lastAt: Int64
    var messageCount: Int
    var unread: Bool
    var flagged: Bool
    var category: MailCategory?
    var summary: String?
    var snippet: String
    var automated: Bool
    var lastFromMe: Bool

    enum CodingKeys: String, CodingKey {
        case id, suspicious, folders, subject, participants, unread, flagged, category, summary, snippet, automated
        case connectionId = "connection_id"
        case receivedOn = "received_on"
        case lastAt = "last_at"
        case messageCount = "message_count"
        case lastFromMe = "last_from_me"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        connectionId = try c.decode(UUID.self, forKey: .connectionId)
        receivedOn = try c.decodeIfPresent(String.self, forKey: .receivedOn)
        suspicious = try c.decodeIfPresent(Bool.self, forKey: .suspicious) ?? false
        folders = try c.decodeIfPresent([Int64].self, forKey: .folders) ?? []
        subject = try c.decodeIfPresent(String.self, forKey: .subject) ?? ""
        participants = try c.decodeIfPresent([MailAddress].self, forKey: .participants) ?? []
        lastAt = try c.decode(Int64.self, forKey: .lastAt)
        messageCount = try c.decodeIfPresent(Int.self, forKey: .messageCount) ?? 1
        unread = try c.decodeIfPresent(Bool.self, forKey: .unread) ?? false
        flagged = try c.decodeIfPresent(Bool.self, forKey: .flagged) ?? false
        // An older app meeting a new category just doesn't show it.
        category = (try? c.decodeIfPresent(MailCategory.self, forKey: .category)) ?? nil
        summary = try c.decodeIfPresent(String.self, forKey: .summary)
        snippet = try c.decodeIfPresent(String.self, forKey: .snippet) ?? ""
        automated = try c.decodeIfPresent(Bool.self, forKey: .automated) ?? false
        lastFromMe = try c.decodeIfPresent(Bool.self, forKey: .lastFromMe) ?? false
    }
}

nonisolated struct MailMessage: Codable, Sendable, Identifiable, Hashable {
    var id: Int64
    var from: MailAddress
    var to: [MailAddress]
    var cc: [MailAddress]
    var date: Int64
    /// Plain text. HTML mail is converted by the computer, hidden text removed.
    var body: String
    var seen: Bool
    var fromMe: Bool
    var attachments: [String]
    var suspicious: Bool
    /// Whether there's an HTML version to show as sent; `nil` until known.
    var hasHtml: Bool?

    enum CodingKeys: String, CodingKey {
        case id, from, to, cc, date, body, seen, attachments, suspicious
        case fromMe = "from_me"
        case hasHtml = "has_html"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        from = try c.decode(MailAddress.self, forKey: .from)
        to = try c.decodeIfPresent([MailAddress].self, forKey: .to) ?? []
        cc = try c.decodeIfPresent([MailAddress].self, forKey: .cc) ?? []
        date = try c.decode(Int64.self, forKey: .date)
        body = try c.decodeIfPresent(String.self, forKey: .body) ?? ""
        seen = try c.decodeIfPresent(Bool.self, forKey: .seen) ?? true
        fromMe = try c.decodeIfPresent(Bool.self, forKey: .fromMe) ?? false
        attachments = try c.decodeIfPresent([String].self, forKey: .attachments) ?? []
        suspicious = try c.decodeIfPresent(Bool.self, forKey: .suspicious) ?? false
        hasHtml = try c.decodeIfPresent(Bool.self, forKey: .hasHtml)
    }
}

nonisolated struct MailThreadDetail: Codable, Sendable {
    var thread: MailThread
    var messages: [MailMessage]
}

/// One email ready to show: its HTML made safe by the computer, and formatted text.
nonisolated struct MailContent: Codable, Sendable, Equatable {
    var html: String?
    var formatted: String
    var remoteImages: Int
    var imagesLoaded: Bool

    enum CodingKeys: String, CodingKey {
        case html, formatted
        case remoteImages = "remote_images"
        case imagesLoaded = "images_loaded"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        html = try c.decodeIfPresent(String.self, forKey: .html)
        formatted = try c.decodeIfPresent(String.self, forKey: .formatted) ?? ""
        remoteImages = try c.decodeIfPresent(Int.self, forKey: .remoteImages) ?? 0
        imagesLoaded = try c.decodeIfPresent(Bool.self, forKey: .imagesLoaded) ?? false
    }
}

nonisolated struct MailReceivedAddress: Codable, Sendable, Hashable {
    var email: String
    var threads: Int
    var unread: Int
}

nonisolated struct MailAccount: Codable, Sendable, Hashable, Identifiable {
    var connectionId: UUID
    var email: String
    var name: String
    var addresses: [MailReceivedAddress]

    var id: UUID { connectionId }

    enum CodingKeys: String, CodingKey {
        case email, name, addresses
        case connectionId = "connection_id"
    }
}

/// A smart folder: the user's sorter files conversations that fit its description.
nonisolated struct MailFolder: Codable, Sendable, Hashable, Identifiable {
    var id: Int64
    var name: String
    var description: String
    var icon: String
    var color: String
    var threads: Int
    var unread: Int
    /// Conversations not yet checked against it (it's still being filled).
    var toCheck: Int

    enum CodingKeys: String, CodingKey {
        case id, name, description, icon, color, threads, unread
        case toCheck = "to_check"
    }
}

/// Counts for the mailboxes list (for the scope asked for), plus the accounts.
nonisolated struct MailOverview: Codable, Sendable {
    var accounts: [MailAccount]
    var needsReply: Int
    var important: Int
    var unread: Int
    var sorting: Bool
    var modelLocality: Locality?
    var sorter: MailSorter
    var sorterLocality: Locality?
    var jevConnected: Bool
    var folders: [MailFolder]

    enum CodingKeys: String, CodingKey {
        case accounts, important, unread, sorting, sorter, folders
        case needsReply = "needs_reply"
        case modelLocality = "model_locality"
        case sorterLocality = "sorter_locality"
        case jevConnected = "jev_connected"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        accounts = try c.decodeIfPresent([MailAccount].self, forKey: .accounts) ?? []
        needsReply = try c.decodeIfPresent(Int.self, forKey: .needsReply) ?? 0
        important = try c.decodeIfPresent(Int.self, forKey: .important) ?? 0
        unread = try c.decodeIfPresent(Int.self, forKey: .unread) ?? 0
        sorting = try c.decodeIfPresent(Bool.self, forKey: .sorting) ?? false
        modelLocality = try? c.decodeIfPresent(Locality.self, forKey: .modelLocality)
        sorter = (try? c.decodeIfPresent(MailSorter.self, forKey: .sorter)) ?? .model
        sorterLocality = try? c.decodeIfPresent(Locality.self, forKey: .sorterLocality)
        jevConnected = try c.decodeIfPresent(Bool.self, forKey: .jevConnected) ?? false
        folders = try c.decodeIfPresent([MailFolder].self, forKey: .folders) ?? []
    }

    /// Every address the user can send from: each account's own and its aliases.
    var sendingAddresses: [(account: UUID, email: String)] {
        accounts.flatMap { a in
            let own = a.addresses.isEmpty ? [a.email] : a.addresses.map(\.email)
            return own.map { (a.connectionId, $0) }
        }
    }

    /// The user's addresses, lowercased: the accounts' own and the aliases mail arrived at.
    var ownAddresses: Set<String> {
        Set(accounts.flatMap { a in [a.email.lowercased()] + a.addresses.map { $0.email.lowercased() } })
    }

    /// The accounts' own addresses: mail to these needs no tag in the list, only aliases do.
    var mainAddresses: Set<String> { Set(accounts.map { $0.email.lowercased() }) }

    /// Whether conversations should say which address they arrived at.
    var manyAddresses: Bool { accounts.reduce(0) { $0 + $1.addresses.count } > 1 }
}

/// A message to send, written by the user (or drafted for them).
nonisolated struct MailDraft: Codable, Sendable, Equatable {
    var connectionId: UUID?
    var from: String?
    var to: [String]
    var cc: [String]
    var subject: String
    var body: String
    var replyTo: Int64?
    var forwardOf: Int64?

    enum CodingKeys: String, CodingKey {
        case from, to, cc, subject, body
        case connectionId = "connection_id"
        case replyTo = "reply_to"
        case forwardOf = "forward_of"
    }

    init(connectionId: UUID? = nil, from: String? = nil, to: [String] = [], cc: [String] = [],
         subject: String = "", body: String = "", replyTo: Int64? = nil, forwardOf: Int64? = nil) {
        self.connectionId = connectionId
        self.from = from
        self.to = to
        self.cc = cc
        self.subject = subject
        self.body = body
        self.replyTo = replyTo
        self.forwardOf = forwardOf
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        connectionId = try? c.decodeIfPresent(UUID.self, forKey: .connectionId)
        from = try c.decodeIfPresent(String.self, forKey: .from)
        to = try c.decodeIfPresent([String].self, forKey: .to) ?? []
        cc = try c.decodeIfPresent([String].self, forKey: .cc) ?? []
        subject = try c.decodeIfPresent(String.self, forKey: .subject) ?? ""
        body = try c.decodeIfPresent(String.self, forKey: .body) ?? ""
        replyTo = try c.decodeIfPresent(Int64.self, forKey: .replyTo)
        forwardOf = try c.decodeIfPresent(Int64.self, forKey: .forwardOf)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        // Lowercase, as the computer writes them.
        try c.encode(connectionId?.uuidString.lowercased(), forKey: .connectionId)
        try c.encode(from, forKey: .from)
        try c.encode(to, forKey: .to)
        try c.encode(cc, forKey: .cc)
        try c.encode(subject, forKey: .subject)
        try c.encode(body, forKey: .body)
        try c.encode(replyTo, forKey: .replyTo)
        try c.encode(forwardOf, forKey: .forwardOf)
    }
}

/// Body of `POST /v1/mail/folders` and `PATCH /v1/mail/folders/{id}`.
nonisolated struct MailFolderInput: Encodable, Sendable {
    var name: String
    var description: String
    var icon: String
    var color: String
}

nonisolated struct MailSummary: Decodable, Sendable {
    var summary: String
}

/// Part of the mail: one account, or one address mail arrived at. Neither is all of it.
nonisolated struct MailScope: Hashable, Sendable, Codable {
    var account: UUID?
    var address: String?

    static let all = MailScope()
    var isAll: Bool { account == nil && address == nil }

    var queryItems: [URLQueryItem] {
        var items: [URLQueryItem] = []
        if let account { items.append(URLQueryItem(name: "account", value: account.uuidString.lowercased())) }
        if let address { items.append(URLQueryItem(name: "address", value: address)) }
        return items
    }
}

/// A connected account's state, from `GET /v1/connections` (only what Mail shows: a
/// mailbox that can't be reached).
nonisolated struct MailConnectionState: Decodable, Sendable {
    var id: UUID
    var integration: String
    var name: String
    var status: String
    var detail: String
}
