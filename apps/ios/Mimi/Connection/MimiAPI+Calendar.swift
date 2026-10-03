import Foundation

/// The Calendar panel's calls, and reminders and routines (`api/calendar.rs`,
/// `api/schedule.rs` on the computer).
nonisolated extension MimiAPI {
    /// An id or query made safe for a path segment or a query value.
    static func escape(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-._~"))) ?? s
    }

    // MARK: Calendars and events

    func calendars() async throws -> [CalendarInfo] { try await call("GET", "/calendars") }

    /// Events of every calendar between two instants (ms), recurrence expanded.
    func events(from: Int64, to: Int64) async throws -> CalendarEvents {
        try await call("GET", "/calendar/events?from=\(from)&to=\(to)")
    }

    func addEvent(_ event: NewCalendarEvent) async throws -> CreatedEvent {
        try await call("POST", "/calendar/events", body: event)
    }

    func changeEvent(_ id: String, _ change: EventChange) async throws -> EventChanged {
        try await call("PATCH", "/calendar/events/\(Self.escape(id))", body: change)
    }

    /// Removes one occurrence, or with `all` the whole series.
    func removeEvent(_ id: String, all: Bool) async throws -> EventChanged {
        try await call("DELETE", "/calendar/events/\(Self.escape(id))\(all ? "?which=all" : "")")
    }

    /// The invitation for an event's guests as it is now, ready to send.
    func offerInvitations(_ eventId: String) async throws -> InvitationOffer {
        try await call("POST", "/calendar/events/\(Self.escape(eventId))/invitations")
    }

    /// The user's own click: sends from their email account.
    func sendInvitations(_ id: UUID) async throws -> InvitationOffer {
        struct Body: Encodable, Sendable { var guests: [String]? = nil }
        return try await call("POST", "/calendar/invitations/\(id.uuidString.lowercased())/send", body: Body())
    }

    func guestSuggestions(_ query: String) async throws -> [GuestSuggestion] {
        try await call("GET", "/calendar/guests?q=\(Self.escape(query))")
    }

    // MARK: Reminders and routines

    func scheduleItems() async throws -> [ScheduleItem] { try await call("GET", "/schedule") }

    func addSchedule(_ item: NewScheduleItem) async throws -> ScheduleItem {
        try await call("POST", "/schedule", body: item)
    }

    func updateSchedule(_ id: UUID, _ update: ScheduleUpdate) async throws -> ScheduleItem {
        try await call("PATCH", "/schedule/\(id.uuidString.lowercased())", body: update)
    }

    func deleteSchedule(_ id: UUID) async throws {
        _ = try await raw("DELETE", "/schedule/\(id.uuidString.lowercased())")
    }

    func runRoutine(_ id: UUID) async throws {
        _ = try await raw("POST", "/schedule/\(id.uuidString.lowercased())/run")
    }

    func deliveries(limit: Int = 30) async throws -> [Delivery] {
        try await call("GET", "/schedule/deliveries?limit=\(limit)")
    }

    /// Every time reminders and routines go off (or went off) in a range.
    func scheduleOccurrences(from: Int64, to: Int64) async throws -> [ScheduleOccurrence] {
        try await call("GET", "/schedule/occurrences?from=\(from)&to=\(to)")
    }

    func reminderDone(_ deliveryId: UUID) async throws {
        _ = try await raw("POST", "/schedule/deliveries/\(deliveryId.uuidString.lowercased())/done")
    }

    func snoozeReminder(_ deliveryId: UUID, minutes: Int) async throws {
        struct Body: Encodable, Sendable { var minutes: Int }
        _ = try await raw("POST", "/schedule/deliveries/\(deliveryId.uuidString.lowercased())/snooze", body: Body(minutes: minutes))
    }

    /// Where reminders reach the user (is a Telegram bot connected).
    func connectionSummaries() async throws -> [ConnectionStatusSummary] { try await call("GET", "/connections") }
}
