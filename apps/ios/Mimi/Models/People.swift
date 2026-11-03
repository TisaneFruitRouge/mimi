import Foundation

// Mirrors of `crates/protocol/src/people.rs`, plus the few things the person page reads
// about someone (events, chats and email with them). Only what the app reads is declared;
// unknown fields are ignored.

/// How a person can be reached. Phone numbers cover calls and SMS.
nonisolated enum Channel: String, Codable, Sendable, CaseIterable, Hashable {
    case phone, email, telegram, signal, whatsapp, matrix, other

    /// Unknown channels from a newer computer read as "other".
    init(from decoder: Decoder) throws {
        self = Channel(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .other
    }
}

/// One way to reach a person, and where Mimi learned it.
nonisolated struct Handle: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var channel: Channel
    var value: String
    var label: String?
    /// A connection's id, or nil when the user added it.
    var sourceId: String?
    /// "iCloud", "Added by you"…
    var sourceName: String

    enum CodingKeys: String, CodingKey {
        case id, channel, value, label
        case sourceId = "source_id"
        case sourceName = "source_name"
    }

    /// Added by the user, so it can be changed or removed here.
    var isOwn: Bool { sourceId == nil }
}

/// A contact card attached to a person; each can be separated again.
nonisolated struct PersonSource: Codable, Sendable, Hashable {
    var sourceId: String?
    var sourceName: String
    var record: String
    var name: String

    enum CodingKeys: String, CodingKey {
        case record, name
        case sourceId = "source_id"
        case sourceName = "source_name"
    }
}

nonisolated struct Person: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var name: String
    var nickname: String?
    var handles: [Handle]
    var sources: [PersonSource]
    var manual: Bool

    enum CodingKeys: String, CodingKey { case id, name, nickname, handles, sources, manual }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        nickname = try c.decodeIfPresent(String.self, forKey: .nickname)
        handles = try c.decodeIfPresent([Handle].self, forKey: .handles) ?? []
        sources = try c.decodeIfPresent([PersonSource].self, forKey: .sources) ?? []
        manual = try c.decodeIfPresent(Bool.self, forKey: .manual) ?? false
    }

    /// What to call them in a sentence: their nickname, else their first name.
    var firstName: String {
        if let nickname, !nickname.isEmpty { return nickname }
        return name.split(separator: " ").first.map(String.init) ?? name
    }

    /// Several contacts in one, each of which can be separated ("Not the same person").
    var isCombined: Bool {
        sources.count + (manual ? 1 : 0) > 1 || sources.contains { $0.sourceId == nil }
    }

    /// "From iCloud", "Added by you", "From iCloud and added by you".
    var sourcesLine: String {
        var names: [String] = []
        for s in sources {
            let n = s.sourceId == nil ? "added by you" : s.sourceName
            if !names.contains(n) { names.append(n) }
        }
        if manual && !names.contains("added by you") { names.append("added by you") }
        if names == ["added by you"] { return "Added by you" }
        return names.isEmpty ? "" : "From " + names.joined(separator: " and ")
    }

    /// Something left of them after a deletion that can be brought back: a card from an
    /// address book.
    var hasImportedCards: Bool { sources.contains { $0.sourceId != nil } }
}

/// A person in lists and pickers.
nonisolated struct PersonSummary: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var name: String
    var nickname: String?
    var channels: [Channel]
    var reach: [String]

    enum CodingKeys: String, CodingKey { case id, name, nickname, channels, reach }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        nickname = try c.decodeIfPresent(String.self, forKey: .nickname)
        channels = try c.decodeIfPresent([Channel].self, forKey: .channels) ?? []
        reach = try c.decodeIfPresent([String].self, forKey: .reach) ?? []
    }

    init(id: UUID, name: String, nickname: String? = nil, channels: [Channel] = [], reach: [String] = []) {
        self.id = id
        self.name = name
        self.nickname = nickname
        self.channels = channels
        self.reach = reach
    }

    /// Numbers and addresses that tell two Sams apart, on one quiet line.
    var reachLine: String {
        if !reach.isEmpty { return reach.joined(separator: " · ") }
        return nickname ?? "No number or address yet"
    }
}

nonisolated struct NewHandle: Codable, Sendable, Hashable {
    var channel: Channel
    var value: String
    var label: String?
}

nonisolated struct NewPerson: Encodable, Sendable {
    var name: String
    var nickname: String?
    var handles: [NewHandle]
}

nonisolated struct PersonUpdate: Encodable, Sendable {
    var name: String?
    /// Empty clears it.
    var nickname: String?
}

nonisolated struct MergeRequest: Encodable, Sendable {
    var keep: UUID
    var others: [UUID]
    var name: String?

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(keep.uuidString.lowercased(), forKey: .keep)
        try c.encode(others.map { $0.uuidString.lowercased() }, forKey: .others)
        try c.encode(name, forKey: .name)
    }

    enum CodingKeys: String, CodingKey { case keep, others, name }
}

/// What a merge would give, for the confirmation.
nonisolated struct MergePreview: Decodable, Sendable {
    var people: [PersonSummary]
    var names: [String]
    var handles: [Handle]
}

nonisolated struct MergeResult: Decodable, Sendable {
    var person: Person
    var mergeId: UUID
    enum CodingKeys: String, CodingKey { case person, mergeId = "merge_id" }
}

/// Two people who may be the same (same name, different cards).
nonisolated struct DuplicateSuggestion: Decodable, Sendable, Identifiable, Hashable {
    var a: PersonSummary
    var b: PersonSummary
    var id: String { "\(a.id)-\(b.id)" }
}

/// Someone deleted from Mimi (not from their address book), who can be brought back.
nonisolated struct RemovedPerson: Decodable, Sendable, Identifiable, Hashable {
    var id: UUID
    var name: String
    var nickname: String?
    var removedAt: Int64
    var sources: [String]

    enum CodingKeys: String, CodingKey {
        case id, name, nickname, sources
        case removedAt = "removed_at"
    }
}

/// A calendar event with someone, as the person page shows it (a `CalendarEvent`).
nonisolated struct PersonEvent: Decodable, Sendable, Identifiable, Hashable {
    var id: String
    var calendarId: String
    var calendar: String
    var title: String
    var start: Int64
    var end: Int64
    var allDay: Bool
    var location: String?

    enum CodingKeys: String, CodingKey {
        case id, calendar, title, start, end, location
        case calendarId = "calendar_id"
        case allDay = "all_day"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        calendarId = try c.decode(String.self, forKey: .calendarId)
        calendar = try c.decodeIfPresent(String.self, forKey: .calendar) ?? ""
        title = try c.decode(String.self, forKey: .title)
        start = try c.decode(Int64.self, forKey: .start)
        end = try c.decode(Int64.self, forKey: .end)
        allDay = try c.decodeIfPresent(Bool.self, forKey: .allDay) ?? false
        location = try c.decodeIfPresent(String.self, forKey: .location)
    }
}

/// A chat where someone was mentioned.
nonisolated struct PersonConversation: Decodable, Sendable, Identifiable, Hashable {
    var id: UUID
    var title: String
    var updatedAt: Int64
    enum CodingKeys: String, CodingKey { case id, title, updatedAt = "updated_at" }
}

/// An email conversation with someone, as the person page shows it (a `MailThread`).
nonisolated struct PersonMailThread: Decodable, Sendable, Identifiable, Hashable {
    var id: Int64
    var subject: String
    var lastAt: Int64
    var unread: Bool
    var summary: String?
    var snippet: String

    enum CodingKeys: String, CodingKey {
        case id, subject, unread, summary, snippet
        case lastAt = "last_at"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        subject = try c.decodeIfPresent(String.self, forKey: .subject) ?? ""
        lastAt = try c.decode(Int64.self, forKey: .lastAt)
        unread = try c.decodeIfPresent(Bool.self, forKey: .unread) ?? false
        summary = try c.decodeIfPresent(String.self, forKey: .summary)
        snippet = try c.decodeIfPresent(String.self, forKey: .snippet) ?? ""
    }
}

/// People sorted into the list's sections: by the first letter of their name, as in
/// Contacts, with anything that doesn't start with a letter under "#".
nonisolated enum PeopleIndex {
    struct Group: Identifiable, Equatable {
        var letter: String
        var people: [PersonSummary]
        var id: String { letter }
    }

    static func letter(of name: String) -> String {
        guard let first = name.trimmingCharacters(in: .whitespaces).first else { return "#" }
        let folded = String(first).folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil).uppercased()
        guard let scalar = folded.unicodeScalars.first, CharacterSet.letters.contains(scalar) else { return "#" }
        return folded
    }

    static func groups(_ people: [PersonSummary]) -> [Group] {
        var order: [String] = []
        var byLetter: [String: [PersonSummary]] = [:]
        for p in people {
            let l = letter(of: p.name)
            if byLetter[l] == nil { order.append(l) }
            byLetter[l, default: []].append(p)
        }
        order.sort { a, b in
            if a == "#" { return false }
            if b == "#" { return true }
            return a.localizedStandardCompare(b) == .orderedAscending
        }
        return order.map { Group(letter: $0, people: byLetter[$0]!) }
    }
}

/// Initials for an avatar: first and last word, "?" when there's nothing.
nonisolated func personInitials(_ name: String) -> String {
    let words = name.split(whereSeparator: \.isWhitespace)
    let picked = words.count > 1 ? [words.first!, words.last!] : words
    let letters = picked.compactMap { $0.first.map { String($0).uppercased() } }.joined()
    return letters.isEmpty ? "?" : letters
}
