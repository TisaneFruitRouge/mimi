import Foundation

/// The "when" part of the reminder form, flat so switching between kinds of repeat keeps
/// what was picked (a port of the desktop's `schedule-dialog.tsx`).
nonisolated struct ScheduleDraft: Equatable, Sendable {
    enum Repeat: String, CaseIterable, Identifiable, Sendable {
        case once, daily, weekdays, weekly, monthly, yearly, interval, beforeEvent

        var id: String { rawValue }

        var label: String {
            switch self {
            case .once: "Once"
            case .daily: "Every day"
            case .weekdays: "Every weekday"
            case .weekly: "Every week"
            case .monthly: "Every month"
            case .yearly: "Every year"
            case .interval: "Every few minutes or hours"
            case .beforeEvent: "Before an event"
            }
        }
    }

    enum Unit: String, CaseIterable, Identifiable, Sendable {
        case minutes, hours
        var id: String { rawValue }
    }

    nonisolated struct EventPick: Equatable, Sendable {
        var id: String
        var title: String
    }

    var repeatKind: Repeat = .once
    /// The date and time for "once", the day for "yearly", the time of day for the rest.
    var date: Date
    var days: Set<Weekday> = [.mon]
    var dayOfMonth: Int
    var every: Int = 30
    var unit: Unit = .minutes
    var event: EventPick?
    var minutesBefore: Int = 10

    /// A new reminder: once, at the next full hour.
    init(now: Date = Date(), math: CalendarMath = CalendarMath()) {
        let next = now.addingTimeInterval(3600)
        var c = math.calendar.dateComponents([.year, .month, .day, .hour], from: next)
        c.minute = 0
        date = math.calendar.date(from: c) ?? next
        dayOfMonth = math.calendar.component(.day, from: now)
    }

    init(schedule: Schedule, now: Date = Date(), math: CalendarMath = CalendarMath()) {
        self.init(now: now, math: math)
        switch schedule {
        case .once(let at):
            repeatKind = .once
            date = math.date(ymd: String(at.prefix(10)), hhmm: String(at.dropFirst(11).prefix(5)))
        case .daily(let time):
            repeatKind = .daily
            date = math.date(ymd: nil, hhmm: time)
        case .weekdays(let time):
            repeatKind = .weekdays
            date = math.date(ymd: nil, hhmm: time)
        case .weekly(let d, let time):
            repeatKind = .weekly
            days = Set(d)
            date = math.date(ymd: nil, hhmm: time)
        case .monthly(let day, let time):
            repeatKind = .monthly
            dayOfMonth = day
            date = math.date(ymd: nil, hhmm: time)
        case .yearly(let month, let day, let time):
            repeatKind = .yearly
            let year = math.calendar.component(.year, from: now)
            date = math.date(ymd: String(format: "%04d-%02d-%02d", year, month, day), hhmm: time)
        case .interval(let minutes):
            repeatKind = .interval
            if minutes % 60 == 0 {
                every = minutes / 60
                unit = .hours
            } else {
                every = minutes
                unit = .minutes
            }
        case .beforeEvent(let id, let title, let before):
            repeatKind = .beforeEvent
            event = EventPick(id: id, title: title)
            minutesBefore = before
        case .unknown:
            break
        }
    }

    /// The rule to save, or nil while something is missing.
    func schedule(math: CalendarMath = CalendarMath()) -> Schedule? {
        let time = math.hhmm(date)
        switch repeatKind {
        case .once: return .once(at: "\(math.ymd(date))T\(time)")
        case .daily: return .daily(time: time)
        case .weekdays: return .weekdays(time: time)
        case .weekly:
            guard !days.isEmpty else { return nil }
            return .weekly(days: Weekday.allCases.filter(days.contains), time: time)
        case .monthly: return .monthly(day: min(31, max(1, dayOfMonth)), time: time)
        case .yearly:
            let c = math.calendar.dateComponents([.month, .day], from: date)
            return .yearly(month: c.month ?? 1, day: c.day ?? 1, time: time)
        case .interval:
            guard every > 0 else { return nil }
            return .interval(minutes: unit == .hours ? every * 60 : every)
        case .beforeEvent:
            guard let event else { return nil }
            return .beforeEvent(eventId: event.id, eventTitle: event.title, minutesBefore: max(0, minutesBefore))
        }
    }
}
