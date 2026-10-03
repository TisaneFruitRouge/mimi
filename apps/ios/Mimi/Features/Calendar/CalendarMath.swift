import Foundation

/// Date arithmetic and wording for the Calendar, a port of the desktop's
/// `features/calendar/dates.ts` and `features/reminders/time.ts`. Pure: everything takes
/// its calendar (time zone, first weekday) and "now", so tests can fix them.
nonisolated struct CalendarMath: Sendable {
    var calendar: Calendar
    var locale: Locale

    init(calendar: Calendar = .current, locale: Locale = .current) {
        var calendar = calendar
        calendar.locale = locale
        self.calendar = calendar
        self.locale = locale
    }

    static let day: Int64 = 86_400_000

    func date(_ ms: Int64) -> Date { Date(timeIntervalSince1970: Double(ms) / 1000) }
    func ms(_ date: Date) -> Int64 { Int64((date.timeIntervalSince1970 * 1000).rounded()) }

    /// Local midnight of the day `ms` falls on.
    func startOfDay(_ ms: Int64) -> Int64 { self.ms(calendar.startOfDay(for: date(ms))) }

    /// Calendar days, not 24-hour steps, so days with a clock change stay aligned.
    func addDays(_ ms: Int64, _ days: Int) -> Int64 {
        self.ms(calendar.date(byAdding: .day, value: days, to: date(ms)) ?? date(ms))
    }

    /// The first day of the week `ms` falls in, as the phone's region counts weeks.
    func startOfWeek(_ ms: Int64) -> Int64 {
        let start = startOfDay(ms)
        let weekday = calendar.component(.weekday, from: date(start))
        let back = (weekday - calendar.firstWeekday + 7) % 7
        return addDays(start, -back)
    }

    func sameDay(_ a: Int64, _ b: Int64) -> Bool { startOfDay(a) == startOfDay(b) }

    /// Whole calendar days from `a`'s day to `b`'s.
    func daysBetween(_ a: Int64, _ b: Int64) -> Int {
        calendar.dateComponents([.day], from: date(startOfDay(a)), to: date(startOfDay(b))).day ?? 0
    }

    /// Whether an event shows on the day starting at `day` (local midnight).
    func onDay(_ e: CalendarEvent, _ day: Int64) -> Bool {
        let next = addDays(day, 1)
        // All-day events end at midnight after their last day.
        return e.start < next && (e.end > day || (e.end == e.start && e.start >= day))
    }

    // MARK: Wording

    /// "9:00", "21:30" or "9:00 PM", as the phone shows times.
    func clock(_ ms: Int64) -> String {
        date(ms).formatted(Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, calendar: calendar, timeZone: calendar.timeZone))
    }

    /// "October 2026".
    func monthTitle(_ ms: Int64) -> String {
        date(ms).formatted(Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone).month(.wide).year())
    }

    /// "Today", "Tomorrow", "Yesterday", else "Wednesday 7 October" (with the year when
    /// it isn't this one).
    func dayTitle(_ ms: Int64, now: Int64) -> String {
        switch daysBetween(now, ms) {
        case 0: return "Today"
        case 1: return "Tomorrow"
        case -1: return "Yesterday"
        default: return longDay(ms, now: now)
        }
    }

    /// "Wednesday 7 October".
    func longDay(_ ms: Int64, now: Int64) -> String {
        var style = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone).weekday(.wide).day().month(.wide)
        if calendar.component(.year, from: date(ms)) != calendar.component(.year, from: date(now)) {
            style = style.year()
        }
        return date(ms).formatted(style)
    }

    /// "9:00 – 9:30", "All day", "All day, 3 days", or across days "Mon 9:00 – Tue 11:00".
    func timeRange(_ e: CalendarEvent) -> String {
        if e.allDay {
            let days = daysBetween(e.start, e.end)
            return days > 1 ? "All day, \(days) days" : "All day"
        }
        if sameDay(e.start, e.end - 1) { return "\(clock(e.start)) – \(clock(e.end))" }
        let weekday = { (ms: Int64) in
            self.date(ms).formatted(Date.FormatStyle(locale: self.locale, calendar: self.calendar, timeZone: self.calendar.timeZone).weekday(.abbreviated))
        }
        return "\(weekday(e.start)) \(clock(e.start)) – \(weekday(e.end)) \(clock(e.end))"
    }

    /// "Wednesday 7 October · 11:00 – 12:00".
    func whenLong(_ e: CalendarEvent, now: Int64) -> String {
        "\(dayTitle(e.start, now: now)) · \(timeRange(e))"
    }

    /// "Today at 9:00", "Tomorrow at 9:00", "Friday at 7:00", "Mon 20 Oct at 9:00".
    func when(_ ms: Int64, now: Int64) -> String {
        let days = daysBetween(now, ms)
        let at = "at \(clock(ms))"
        switch days {
        case 0: return "Today \(at)"
        case 1: return "Tomorrow \(at)"
        case -1: return "Yesterday \(at)"
        case 2..<7:
            return "\(date(ms).formatted(Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone).weekday(.wide))) \(at)"
        default:
            var style = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone).weekday(.abbreviated).day().month(.abbreviated)
            if calendar.component(.year, from: date(ms)) != calendar.component(.year, from: date(now)) { style = style.year() }
            return "\(date(ms).formatted(style)) \(at)"
        }
    }

    /// An event's details as plain text, for Copy.
    func eventText(_ e: CalendarEvent, now: Int64) -> String {
        [e.title, whenLong(e, now: now), e.location].compactMap { $0 }.joined(separator: "\n")
    }

    // MARK: The day timeline

    /// The shortest an event is drawn, so its title fits on one line.
    static let minMinutes: Double = 28

    nonisolated struct Placed: Sendable, Equatable {
        var event: CalendarEvent
        /// Minutes from the day's midnight, clipped to the day.
        var top: Double
        var bottom: Double
        var column: Int
        var columns: Int
    }

    /// Timed events of one day, side by side where they overlap: each cluster of
    /// overlapping events gets as many columns as it needs, like Calendar.
    func layoutDay(_ events: [CalendarEvent], day: Int64) -> [Placed] {
        let end = addDays(day, 1)
        let minutes = { (ms: Int64) in Double(ms - day) / 60_000 }
        var items = events
            .filter { !$0.allDay && $0.start < end && $0.end > day }
            .map { e -> Placed in
                let top = max(0, minutes(e.start))
                let bottom = max(top + Self.minMinutes, min(minutes(end), minutes(e.end)))
                return Placed(event: e, top: top, bottom: bottom, column: 0, columns: 1)
            }
            .sorted { $0.top != $1.top ? $0.top < $1.top : $0.bottom > $1.bottom }

        var cluster: [Int] = []
        var clusterEnd = -1.0
        var columnEnds: [Double] = []
        func close() {
            let n = max(1, (cluster.map { items[$0].column + 1 }.max() ?? 1))
            for i in cluster { items[i].columns = n }
            cluster = []
            columnEnds = []
        }
        for i in items.indices {
            if items[i].top >= clusterEnd && !cluster.isEmpty { close() }
            let top = items[i].top
            let col: Int
            if let free = columnEnds.firstIndex(where: { $0 <= top }) {
                col = free
                columnEnds[col] = items[i].bottom
            } else {
                col = columnEnds.count
                columnEnds.append(items[i].bottom)
            }
            items[i].column = col
            cluster.append(i)
            clusterEnd = max(clusterEnd, items[i].bottom)
        }
        if !cluster.isEmpty { close() }
        return items
    }

    /// Where reminder chips sit on a day (in minutes), each at its time but kept below
    /// the one before it, so reminders a few minutes apart don't hide each other.
    func chipTops(_ times: [Int64], day: Int64, chipMinutes: Double) -> [Double] {
        var out: [Double] = []
        for t in times {
            let ideal = max(0, Double(t - day) / 60_000 - chipMinutes / 2)
            let placed = out.last.map { max(ideal, $0 + chipMinutes + 1) } ?? ideal
            out.append(min(24 * 60 - chipMinutes, placed))
        }
        return out
    }

    // MARK: Schedules

    /// "HH:MM" of a date, as schedules store times.
    func hhmm(_ d: Date) -> String {
        let c = calendar.dateComponents([.hour, .minute], from: d)
        return String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
    }

    /// "YYYY-MM-DD" of a date.
    func ymd(_ d: Date) -> String {
        let c = calendar.dateComponents([.year, .month, .day], from: d)
        return String(format: "%04d-%02d-%02d", c.year ?? 2000, c.month ?? 1, c.day ?? 1)
    }

    /// A date on `ymd`'s day (or today) at `hhmm`, for the pickers.
    func date(ymd: String?, hhmm: String) -> Date {
        var c = DateComponents()
        if let ymd {
            let p = ymd.split(separator: "-").compactMap { Int($0) }
            if p.count == 3 { c.year = p[0]; c.month = p[1]; c.day = p[2] }
        }
        if c.year == nil {
            let today = calendar.dateComponents([.year, .month, .day], from: Date())
            c.year = today.year; c.month = today.month; c.day = today.day
        }
        let t = hhmm.split(separator: ":").compactMap { Int($0) }
        c.hour = t.first ?? 9
        c.minute = t.count > 1 ? t[1] : 0
        return calendar.date(from: c) ?? Date()
    }
}

/// Wording shared by the reminder screens.
nonisolated enum ScheduleWords {
    /// "Remind me" choices for an event (the reminder follows the event if it moves).
    static let before: [(minutes: Int, label: String)] = [
        (10, "10 minutes before"),
        (30, "30 minutes before"),
        (60, "1 hour before"),
        (24 * 60, "1 day before"),
    ]

    static func beforeLabel(_ minutes: Int) -> String {
        if let b = before.first(where: { $0.minutes == minutes }) { return b.label }
        if minutes == 0 { return "When it starts" }
        if minutes % (24 * 60) == 0 { let d = minutes / (24 * 60); return d == 1 ? "1 day before" : "\(d) days before" }
        if minutes % 60 == 0 { let h = minutes / 60; return h == 1 ? "1 hour before" : "\(h) hours before" }
        return minutes == 1 ? "1 minute before" : "\(minutes) minutes before"
    }

    /// What happened to a reminder that went off, in plain words.
    static func status(_ s: DeliveryStatus) -> String {
        switch s {
        case .delivered: "Sent"
        case .late: "Sent late"
        case .missed: "Missed"
        case .done: "Done"
        case .snoozed: "Snoozed"
        case .running: "Running…"
        case .failed: "Didn't finish"
        case .skipped: "Skipped"
        case .unknown: ""
        }
    }

    /// One line under a reminder in the list: when it's next, or why it won't be.
    static func detail(_ i: ScheduleItem, math: CalendarMath, now: Int64) -> String {
        if let ended = i.ended { return ended }
        if i.finished { return i.lastAt.map { "Went off \(math.when($0, now: now).lowerFirst)" } ?? "Done" }
        if i.paused { return i.description }
        guard let next = i.nextAt else { return i.description }
        return i.schedule.repeats ? "\(math.when(next, now: now)) · \(i.description)" : math.when(next, now: now)
    }

    /// "Sam", "Sam and Léa", "Sam, Léa and Tom".
    static func joinNames(_ names: [String]) -> String {
        guard names.count > 1 else { return names.first ?? "" }
        return names.dropLast().joined(separator: ", ") + " and " + names.last!
    }

    static func guestNames(_ o: InvitationOffer) -> String { joinNames(o.guests.map { $0.name ?? $0.email }) }

    /// What the button says it will do.
    static func offerLabel(_ o: InvitationOffer) -> String {
        let who = guestNames(o)
        switch o.kind {
        case .invite, .unknown: return o.guests.count == 1 ? "Send the invitation to \(who)" : "Send invitations to \(who)"
        case .update: return "Let \(who) know about the change"
        case .cancel: return "Tell \(who) it's cancelled"
        case .uninvite: return "Withdraw the invitation for \(who)"
        }
    }

    /// What happened once it was sent.
    static func sentLabel(_ o: InvitationOffer) -> String {
        let who = guestNames(o)
        switch o.kind {
        case .invite, .unknown: return o.guests.count == 1 ? "Invitation sent to \(who)" : "Invitations sent to \(who)"
        case .update: return "\(who) got the new details"
        case .cancel: return "Told \(who) it's cancelled"
        case .uninvite: return "Invitation withdrawn for \(who)"
        }
    }

    /// The question asked about an offer once the event is gone from the screen.
    static func question(_ o: InvitationOffer) -> String {
        switch o.kind {
        case .invite, .unknown: "Send them the invitation?"
        case .update: "Let them know about the change?"
        case .cancel: "Tell them it's cancelled?"
        case .uninvite: "Tell them they're no longer invited?"
        }
    }
}

/// Someone invited: a name when known, and the address the invitation goes to.
nonisolated struct GuestEntry: Hashable, Sendable, Identifiable {
    var name: String?
    var email: String
    var id: String { email }

    /// What the computer reads: "Sam Carter <sam@example.com>" or a bare address.
    var text: String {
        guard let name, !name.isEmpty else { return email }
        let clean = name.filter { !"<>\",;".contains($0) }
        return "\(clean) <\(email)>"
    }

    /// An address someone could have typed out in full.
    static func isEmail(_ s: String) -> Bool {
        let forbidden = CharacterSet(charactersIn: " @<>\",;:").union(.whitespacesAndNewlines)
        let parts = s.split(separator: "@", omittingEmptySubsequences: false)
        guard parts.count == 2, !parts[0].isEmpty, !parts[1].isEmpty else { return false }
        let local = String(parts[0]), domain = String(parts[1])
        guard local.rangeOfCharacter(from: forbidden) == nil, domain.rangeOfCharacter(from: forbidden) == nil else { return false }
        let labels = domain.split(separator: ".", omittingEmptySubsequences: false)
        return labels.count >= 2 && labels.allSatisfy { !$0.isEmpty }
    }
}
