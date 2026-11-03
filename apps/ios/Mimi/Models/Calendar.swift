import Foundation

// Mirrors of `crates/protocol/src/calendar.rs`. Titles, places, notes and names come from
// calendars other people can write into: show them as plain text, never as Markdown or
// links.

nonisolated struct CalendarInfo: Codable, Sendable, Identifiable, Hashable {
    var id: String
    var name: String
    /// `#rrggbb`: the calendar's own colour, else a stable pick.
    var color: String
    /// Events are saved straight into it. Google calendars read by address aren't: new
    /// events open in Google Calendar for the user to save.
    var writable: Bool
    var google: Bool
    /// People can be invited to its events.
    var guests: Bool

    enum CodingKeys: String, CodingKey { case id, name, color, writable, google, guests }

    init(id: String, name: String, color: String, writable: Bool, google: Bool = false, guests: Bool = false) {
        self.id = id
        self.name = name
        self.color = color
        self.writable = writable
        self.google = google
        self.guests = guests
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        color = try c.decodeIfPresent(String.self, forKey: .color) ?? "#8e8e93"
        writable = try c.decodeIfPresent(Bool.self, forKey: .writable) ?? false
        google = try c.decodeIfPresent(Bool.self, forKey: .google) ?? false
        guests = try c.decodeIfPresent(Bool.self, forKey: .guests) ?? false
    }
}

/// A guest's answer to an invitation.
nonisolated enum GuestResponse: String, Codable, Sendable, Hashable {
    case accepted, declined, tentative, pending, unknown

    init(from decoder: Decoder) throws {
        self = GuestResponse(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }

    /// In plain words.
    var label: String? {
        switch self {
        case .accepted: "Going"
        case .declined: "Not going"
        case .tentative: "Maybe"
        case .pending: "No answer yet"
        case .unknown: nil
        }
    }
}

/// Someone on an event (organizer or guest), matched to People when possible.
nonisolated struct EventPerson: Codable, Sendable, Hashable {
    var name: String?
    var email: String
    var personId: UUID?
    var personName: String?
    var response: GuestResponse?

    enum CodingKeys: String, CodingKey {
        case name, email, response
        case personId = "person_id"
        case personName = "person_name"
    }

    /// The name People knows them by, else the invitation's, else their address.
    var displayName: String { personName ?? name ?? email }
}

/// One occurrence of an event.
nonisolated struct CalendarEvent: Codable, Sendable, Identifiable, Hashable {
    /// Stable per occurrence; what @ mentions and "remind me before" use.
    var id: String
    var calendarId: String
    var calendar: String
    var title: String
    var start: Int64
    var end: Int64
    var allDay: Bool
    var location: String?
    var notes: String?
    var organizer: EventPerson?
    var attendees: [EventPerson]
    /// One occurrence of a repeating event.
    var repeats: Bool
    /// The user organizes it (or nobody does): they may change its guests.
    var mine: Bool

    enum CodingKeys: String, CodingKey {
        case id, calendar, title, start, end, location, notes, organizer, attendees, repeats, mine
        case calendarId = "calendar_id"
        case allDay = "all_day"
    }

    init(
        id: String, calendarId: String = "c", calendar: String = "Home", title: String,
        start: Int64, end: Int64, allDay: Bool = false, location: String? = nil, notes: String? = nil,
        organizer: EventPerson? = nil, attendees: [EventPerson] = [], repeats: Bool = false, mine: Bool = true
    ) {
        self.id = id
        self.calendarId = calendarId
        self.calendar = calendar
        self.title = title
        self.start = start
        self.end = end
        self.allDay = allDay
        self.location = location
        self.notes = notes
        self.organizer = organizer
        self.attendees = attendees
        self.repeats = repeats
        self.mine = mine
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
        location = (try c.decodeIfPresent(String.self, forKey: .location)).flatMap { $0.isEmpty ? nil : $0 }
        notes = (try c.decodeIfPresent(String.self, forKey: .notes)).flatMap { $0.isEmpty ? nil : $0 }
        organizer = try c.decodeIfPresent(EventPerson.self, forKey: .organizer)
        attendees = try c.decodeIfPresent([EventPerson].self, forKey: .attendees) ?? []
        repeats = try c.decodeIfPresent(Bool.self, forKey: .repeats) ?? false
        mine = try c.decodeIfPresent(Bool.self, forKey: .mine) ?? false
    }

    /// Guests other than the organizer (the user, on their own events).
    var guests: [EventPerson] {
        attendees.filter { $0.email != organizer?.email }
    }

    /// "Send invitations…" makes sense: the user's own event, with guests.
    var canInvite: Bool { mine && !guests.isEmpty }

    /// Organizer first, then guests, each address once.
    var people: [(person: EventPerson, organizer: Bool)] {
        var seen = Set<String>()
        var out: [(EventPerson, Bool)] = []
        for (p, organizer) in (organizer.map { [($0, true)] } ?? []) + attendees.map({ ($0, false) }) {
            guard seen.insert(p.email).inserted else { continue }
            out.append((p, organizer))
        }
        return out
    }
}

nonisolated struct CalendarEvents: Codable, Sendable {
    var events: [CalendarEvent]
    /// Calendars that couldn't be read just now, with why, in plain words.
    var unavailable: [String]

    enum CodingKeys: String, CodingKey { case events, unavailable }

    init(events: [CalendarEvent] = [], unavailable: [String] = []) {
        self.events = events
        self.unavailable = unavailable
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        events = try c.decodeIfPresent([CalendarEvent].self, forKey: .events) ?? []
        unavailable = try c.decodeIfPresent([String].self, forKey: .unavailable) ?? []
    }
}

/// An event the user adds.
nonisolated struct NewCalendarEvent: Encodable, Sendable {
    var calendarId: String
    var title: String
    /// For all-day events, local midnight of the first day.
    var start: Int64
    /// For all-day events, local midnight after the last day.
    var end: Int64
    var allDay: Bool
    var location: String?
    var notes: String?
    /// "Sam <sam@example.com>" or bare addresses. Nobody is emailed.
    var guests: [String]

    enum CodingKeys: String, CodingKey {
        case title, start, end, location, notes, guests
        case calendarId = "calendar_id"
        case allDay = "all_day"
    }
}

/// A change to one occurrence. Fields left out stay as they are; an empty place or notes
/// removes it.
nonisolated struct EventChange: Encodable, Sendable {
    var title: String?
    var start: Int64?
    var end: Int64?
    var allDay: Bool?
    var location: String?
    var notes: String?
    /// Everyone who should be a guest afterwards. Absent: unchanged.
    var guests: [String]?

    enum CodingKeys: String, CodingKey {
        case title, start, end, location, notes, guests
        case allDay = "all_day"
    }
}

nonisolated enum InvitationKind: String, Codable, Sendable {
    case invite, update, cancel, uninvite, unknown

    init(from decoder: Decoder) throws {
        self = InvitationKind(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }
}

nonisolated struct InvitationGuest: Codable, Sendable, Hashable {
    var name: String?
    var email: String
}

/// Invitation emails Mimi can send for an event, from the user's own email account. Only
/// ever sent by the user's click.
nonisolated struct InvitationOffer: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var kind: InvitationKind
    var eventTitle: String
    var eventWhen: String
    var guests: [InvitationGuest]
    /// The address it's sent from; nil while no email account is connected.
    var from: String?
    var fromNote: String?
    var sentAt: Int64?

    enum CodingKeys: String, CodingKey {
        case id, kind, guests, from
        case eventTitle = "event_title"
        case eventWhen = "event_when"
        case fromNote = "from_note"
        case sentAt = "sent_at"
    }
}

/// What happened to a new event.
nonisolated struct CreatedEvent: Decodable, Sendable {
    var saved: Bool
    var calendar: String
    /// For Google calendars read by address: the pre-filled page where the user presses Save.
    var openURL: String?
    var invitations: [InvitationOffer]
    var note: String?

    enum CodingKeys: String, CodingKey {
        case saved, calendar, invitations, note
        case openURL = "open_url"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        saved = try c.decodeIfPresent(Bool.self, forKey: .saved) ?? false
        calendar = try c.decodeIfPresent(String.self, forKey: .calendar) ?? ""
        openURL = try c.decodeIfPresent(String.self, forKey: .openURL)
        invitations = try c.decodeIfPresent([InvitationOffer].self, forKey: .invitations) ?? []
        note = try c.decodeIfPresent(String.self, forKey: .note)
    }
}

/// What happened to a changed or removed event.
nonisolated struct EventChanged: Decodable, Sendable {
    var eventId: String?
    var invitations: [InvitationOffer]
    var note: String?

    enum CodingKeys: String, CodingKey {
        case invitations, note
        case eventId = "event_id"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        eventId = try c.decodeIfPresent(String.self, forKey: .eventId)
        invitations = try c.decodeIfPresent([InvitationOffer].self, forKey: .invitations) ?? []
        note = try c.decodeIfPresent(String.self, forKey: .note)
    }
}

/// Someone who could be invited: a person in People, by one of their addresses.
nonisolated struct GuestSuggestion: Codable, Sendable, Hashable, Identifiable {
    var personId: UUID
    var name: String
    var email: String

    var id: String { "\(personId)-\(email)" }

    enum CodingKeys: String, CodingKey {
        case name, email
        case personId = "person_id"
    }
}
