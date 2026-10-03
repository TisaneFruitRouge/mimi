import Foundation

// Mirrors of `crates/protocol/src/schedule.rs`: reminders and routines.

nonisolated enum ScheduleKind: String, Codable, Sendable, Hashable {
    /// Tells the user something at the right time.
    case reminder
    /// The assistant carries out an instruction on a schedule and reports back.
    case routine

    init(from decoder: Decoder) throws {
        self = ScheduleKind(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .reminder
    }
}

nonisolated enum Weekday: String, Codable, Sendable, Hashable, CaseIterable, Identifiable {
    case mon, tue, wed, thu, fri, sat, sun

    var id: String { rawValue }

    /// "Mon", in the phone's language.
    var shortName: String {
        // Calendar's weekday symbols start on Sunday.
        Calendar.current.shortWeekdaySymbols[(Weekday.allCases.firstIndex(of: self)! + 1) % 7]
    }

    /// "M", "T"…, for the day toggles.
    var letter: String {
        Calendar.current.veryShortWeekdaySymbols[(Weekday.allCases.firstIndex(of: self)! + 1) % 7]
    }
}

/// When something happens: a rule, not instants, in the computer's own time zone. Times
/// are local `HH:MM`, dates local `YYYY-MM-DD`.
nonisolated enum Schedule: Codable, Sendable, Hashable {
    /// Once, at a local date and time (`YYYY-MM-DDTHH:MM`).
    case once(at: String)
    case daily(time: String)
    /// Monday to Friday.
    case weekdays(time: String)
    case weekly(days: [Weekday], time: String)
    /// Months that are too short use their last day.
    case monthly(day: Int, time: String)
    case yearly(month: Int, day: Int, time: String)
    /// Every so many minutes, counted from when it was set up.
    case interval(minutes: Int)
    /// Some time before a calendar event, following it if it moves.
    case beforeEvent(eventId: String, eventTitle: String, minutesBefore: Int)
    /// A kind of rule this app doesn't know yet: shown, not edited.
    case unknown(String)

    private enum Keys: String, CodingKey {
        case type, at, time, days, day, month, minutes
        case eventId = "event_id"
        case eventTitle = "event_title"
        case minutesBefore = "minutes_before"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "once": self = .once(at: try c.decode(String.self, forKey: .at))
        case "daily": self = .daily(time: try c.decode(String.self, forKey: .time))
        case "weekdays": self = .weekdays(time: try c.decode(String.self, forKey: .time))
        case "weekly":
            // A day this app doesn't know is left out rather than failing the whole list.
            let days = (try c.decode([String].self, forKey: .days)).compactMap(Weekday.init(rawValue:))
            self = .weekly(days: days, time: try c.decode(String.self, forKey: .time))
        case "monthly": self = .monthly(day: try c.decode(Int.self, forKey: .day), time: try c.decode(String.self, forKey: .time))
        case "yearly":
            self = .yearly(
                month: try c.decode(Int.self, forKey: .month),
                day: try c.decode(Int.self, forKey: .day),
                time: try c.decode(String.self, forKey: .time)
            )
        case "interval": self = .interval(minutes: try c.decode(Int.self, forKey: .minutes))
        case "before_event":
            self = .beforeEvent(
                eventId: try c.decode(String.self, forKey: .eventId),
                eventTitle: try c.decodeIfPresent(String.self, forKey: .eventTitle) ?? "",
                minutesBefore: try c.decode(Int.self, forKey: .minutesBefore)
            )
        default: self = .unknown(type)
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .once(let at):
            try c.encode("once", forKey: .type)
            try c.encode(at, forKey: .at)
        case .daily(let time):
            try c.encode("daily", forKey: .type)
            try c.encode(time, forKey: .time)
        case .weekdays(let time):
            try c.encode("weekdays", forKey: .type)
            try c.encode(time, forKey: .time)
        case .weekly(let days, let time):
            try c.encode("weekly", forKey: .type)
            try c.encode(days, forKey: .days)
            try c.encode(time, forKey: .time)
        case .monthly(let day, let time):
            try c.encode("monthly", forKey: .type)
            try c.encode(day, forKey: .day)
            try c.encode(time, forKey: .time)
        case .yearly(let month, let day, let time):
            try c.encode("yearly", forKey: .type)
            try c.encode(month, forKey: .month)
            try c.encode(day, forKey: .day)
            try c.encode(time, forKey: .time)
        case .interval(let minutes):
            try c.encode("interval", forKey: .type)
            try c.encode(minutes, forKey: .minutes)
        case .beforeEvent(let eventId, let eventTitle, let minutesBefore):
            try c.encode("before_event", forKey: .type)
            try c.encode(eventId, forKey: .eventId)
            try c.encode(eventTitle, forKey: .eventTitle)
            try c.encode(minutesBefore, forKey: .minutesBefore)
        case .unknown:
            throw EncodingError.invalidValue(self, .init(codingPath: encoder.codingPath, debugDescription: "unknown schedule"))
        }
    }

    var isBeforeEvent: Bool { if case .beforeEvent = self { true } else { false } }
    var repeats: Bool {
        switch self {
        case .once, .beforeEvent: false
        default: true
        }
    }
}

nonisolated struct ScheduleItem: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var kind: ScheduleKind
    /// What to remind about, or the routine's name.
    var title: String
    /// For routines: what the assistant does each time.
    var instruction: String?
    var schedule: Schedule
    /// The schedule in words, e.g. "Every weekday at 07:00".
    var description: String
    var paused: Bool
    /// When it happens next, if ever. Includes snoozes.
    var nextAt: Int64?
    var lastAt: Int64?
    /// Why it won't happen again. nil while active.
    var ended: String?
    /// For routines: the conversation their results go to.
    var conversationId: UUID?
    var createdAt: Int64

    enum CodingKeys: String, CodingKey {
        case id, kind, title, instruction, schedule, description, paused, ended
        case nextAt = "next_at"
        case lastAt = "last_at"
        case conversationId = "conversation_id"
        case createdAt = "created_at"
    }

    init(
        id: UUID = UUID(), kind: ScheduleKind = .reminder, title: String, instruction: String? = nil,
        schedule: Schedule, description: String = "", paused: Bool = false, nextAt: Int64? = nil,
        lastAt: Int64? = nil, ended: String? = nil, conversationId: UUID? = nil, createdAt: Int64 = 0
    ) {
        self.id = id
        self.kind = kind
        self.title = title
        self.instruction = instruction
        self.schedule = schedule
        self.description = description
        self.paused = paused
        self.nextAt = nextAt
        self.lastAt = lastAt
        self.ended = ended
        self.conversationId = conversationId
        self.createdAt = createdAt
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        kind = try c.decode(ScheduleKind.self, forKey: .kind)
        title = try c.decode(String.self, forKey: .title)
        instruction = try c.decodeIfPresent(String.self, forKey: .instruction)
        schedule = try c.decode(Schedule.self, forKey: .schedule)
        description = try c.decodeIfPresent(String.self, forKey: .description) ?? ""
        paused = try c.decodeIfPresent(Bool.self, forKey: .paused) ?? false
        nextAt = try c.decodeIfPresent(Int64.self, forKey: .nextAt)
        lastAt = try c.decodeIfPresent(Int64.self, forKey: .lastAt)
        ended = try c.decodeIfPresent(String.self, forKey: .ended)
        conversationId = try c.decodeIfPresent(UUID.self, forKey: .conversationId)
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
    }

    /// Won't happen again: a one-time reminder that went off, or one whose event is gone.
    var finished: Bool { ended != nil || (nextAt == nil && !paused) }
}

nonisolated enum DeliveryStatus: String, Codable, Sendable, Hashable {
    case delivered, late, missed, done, snoozed, running, failed, skipped, unknown

    init(from decoder: Decoder) throws {
        self = DeliveryStatus(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }
}

/// One time a reminder went off or a routine ran.
nonisolated struct Delivery: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var itemId: UUID
    var kind: ScheduleKind
    var title: String
    /// The time it was due.
    var dueAt: Int64
    /// When it actually happened.
    var at: Int64
    var status: DeliveryStatus
    var detail: String?
    /// For routines: where the result is.
    var conversationId: UUID?

    enum CodingKeys: String, CodingKey {
        case id, kind, title, at, status, detail
        case itemId = "item_id"
        case dueAt = "due_at"
        case conversationId = "conversation_id"
    }
}

nonisolated struct NewScheduleItem: Encodable, Sendable {
    var kind: ScheduleKind
    var title: String
    var instruction: String?
    var schedule: Schedule
}

/// Fields left nil are unchanged.
nonisolated struct ScheduleUpdate: Encodable, Sendable {
    var title: String?
    var instruction: String?
    var schedule: Schedule?
    var paused: Bool?
}

/// One time a reminder or routine goes off (or went off), for the calendar. Items that
/// repeat every few minutes come once per day, standing for `count` times.
nonisolated struct ScheduleOccurrence: Codable, Sendable, Hashable {
    var itemId: UUID
    var at: Int64
    var until: Int64
    var count: Int
    /// What happened, for one that already came due; nil for one still to come.
    var status: DeliveryStatus?
    /// When a snoozed reminder comes back.
    var snoozed: Bool

    enum CodingKeys: String, CodingKey {
        case at, until, count, status, snoozed
        case itemId = "item_id"
    }

    init(itemId: UUID, at: Int64, until: Int64? = nil, count: Int = 1, status: DeliveryStatus? = nil, snoozed: Bool = false) {
        self.itemId = itemId
        self.at = at
        self.until = until ?? at
        self.count = count
        self.status = status
        self.snoozed = snoozed
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        itemId = try c.decode(UUID.self, forKey: .itemId)
        at = try c.decode(Int64.self, forKey: .at)
        until = try c.decodeIfPresent(Int64.self, forKey: .until) ?? at
        count = try c.decodeIfPresent(Int.self, forKey: .count) ?? 1
        status = try c.decodeIfPresent(DeliveryStatus.self, forKey: .status)
        snoozed = try c.decodeIfPresent(Bool.self, forKey: .snoozed) ?? false
    }
}

/// The part of a connection the Reminders & notifications page reads: is Telegram there.
nonisolated struct ConnectionStatusSummary: Decodable, Sendable {
    var integration: String
    var status: String
}
