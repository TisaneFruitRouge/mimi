import Foundation

/// How each tool's arguments read on an approval card: the same rules as the desktop's
/// `features/chat/action-formatters.tsx`. Anything without a formatter is a tidy list of
/// its arguments, and an argument a formatter doesn't cover is still shown after its rows.
nonisolated enum ActionArguments {
    struct Row: Hashable, Sendable {
        var label: String
        var value: String
    }

    private struct Formatter: Sendable {
        var keys: Set<String>
        var rows: @Sendable ([String: JSONValue]) -> [Row]
    }

    static func rows(tool: String, arguments: JSONValue) -> [Row] {
        let all = arguments.object ?? ["arguments": arguments]
        let format = formatters[tool]
        var rows = format?.rows(all) ?? []
        let covered = format?.keys ?? []
        for key in all.keys.sorted() where !covered.contains(key) && present(all[key]) {
            rows.append(Row(label: humanize(key), value: text(all[key])))
        }
        return rows
    }

    private static let formatters: [String: Formatter] = [
        "calendar_add_event": Formatter(
            keys: ["title", "start", "end", "calendar", "location", "notes", "guests", "guests_note"]
        ) { a in
            var rows = [Row(label: "Event", value: text(a["title"]))]
            rows.append(Row(label: "When", value: when(text(a["start"]), present(a["end"]) ? text(a["end"]) : nil)))
            if present(a["calendar"]) { rows.append(Row(label: "Calendar", value: text(a["calendar"]))) }
            if present(a["location"]) { rows.append(Row(label: "Where", value: text(a["location"]))) }
            if present(a["guests"]) { rows.append(Row(label: "Guests", value: lines(a["guests"]))) }
            if present(a["guests_note"]) { rows.append(Row(label: "Guests", value: text(a["guests_note"]))) }
            if present(a["notes"]) { rows.append(Row(label: "Notes", value: text(a["notes"]))) }
            if present(a["guests"]) {
                rows.append(Row(label: "Email", value: "Nobody is emailed. You can send the invitations after."))
            }
            return rows
        },
        "calendar_change_event": Formatter(
            keys: ["event", "event_title", "event_when", "calendar", "calendar_id", "repeats", "which", "new_time",
                   "title", "start", "end", "location", "notes", "add_guests", "remove_guests", "guests"]
        ) { a in
            var rows = [
                Row(label: "Event", value: text(a["event_title"])),
                Row(label: "Now", value: text(a["event_when"]).upperFirst),
                Row(label: "Calendar", value: text(a["calendar"])),
            ]
            if a["repeats"]?.bool == true {
                rows.append(Row(label: "Repeats", value: a["which"]?.string == "all" ? "Change every time" : "Change only this time"))
            }
            if present(a["new_time"]) { rows.append(Row(label: "New time", value: text(a["new_time"]).upperFirst)) }
            if present(a["title"]) { rows.append(Row(label: "New name", value: text(a["title"]))) }
            if let location = a["location"]?.string {
                rows.append(Row(label: "Where", value: location.trimmingCharacters(in: .whitespaces).isEmpty ? "(removed)" : location))
            }
            if let notes = a["notes"]?.string {
                rows.append(Row(label: "Notes", value: notes.trimmingCharacters(in: .whitespaces).isEmpty ? "(removed)" : notes))
            }
            if present(a["add_guests"]) { rows.append(Row(label: "Invite", value: lines(a["add_guests"]))) }
            if present(a["remove_guests"]) { rows.append(Row(label: "Take off", value: lines(a["remove_guests"]))) }
            if present(a["guests"]) { rows.append(Row(label: "Guests after", value: lines(a["guests"]))) }
            if present(a["add_guests"]) || present(a["remove_guests"]) {
                rows.append(Row(label: "Email", value: "Nobody is emailed. You can tell them after."))
            }
            return rows
        },
        "calendar_send_invitations": Formatter(
            keys: ["event", "offer", "kind", "event_title", "event_when", "recipients", "from", "from_note", "guests"]
        ) { a in
            let what = [
                "invite": "The invitation",
                "update": "The new details",
                "cancel": "It's cancelled",
                "uninvite": "They're no longer invited",
            ]
            var rows = [
                Row(label: "Event", value: text(a["event_title"])),
                Row(label: "When", value: text(a["event_when"]).upperFirst),
                Row(label: "To", value: lines(a["recipients"])),
                Row(label: "Message", value: what[text(a["kind"])] ?? "The invitation"),
            ]
            if present(a["from"]) { rows.append(Row(label: "From", value: text(a["from"]))) }
            if present(a["from_note"]) { rows.append(Row(label: "Note", value: text(a["from_note"]))) }
            return rows
        },
        "calendar_delete_event": Formatter(
            keys: ["event", "event_title", "event_when", "calendar", "calendar_id", "repeats", "which"]
        ) { a in
            var rows = [
                Row(label: "Event", value: text(a["event_title"])),
                Row(label: "When", value: text(a["event_when"]).upperFirst),
                Row(label: "Calendar", value: text(a["calendar"])),
            ]
            if a["repeats"]?.bool == true {
                rows.append(Row(label: "Repeats", value: a["which"]?.string == "all" ? "Remove every time" : "Remove only this time"))
            }
            return rows
        },
        "mail_send": Formatter(keys: ["to", "cc", "subject", "body", "thread_id"]) { a in
            var rows = [Row(label: "To", value: text(a["to"]))]
            if present(a["cc"]) { rows.append(Row(label: "Cc", value: text(a["cc"]))) }
            let subject = text(a["subject"])
            rows.append(Row(label: "Subject", value: subject.isEmpty ? "(no subject)" : subject))
            rows.append(Row(label: "Message", value: text(a["body"])))
            return rows
        },
    ]

    static func text(_ v: JSONValue?) -> String {
        guard let v else { return "" }
        switch v {
        case .array(let items): return items.map { text($0) }.joined(separator: ", ")
        case .object:
            guard let data = try? JSONEncoder().encode(v) else { return "" }
            return String(decoding: data, as: UTF8.self)
        default: return v.display
        }
    }

    static func present(_ v: JSONValue?) -> Bool {
        !text(v).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// One entry per line: each guest or recipient on their own.
    static func lines(_ v: JSONValue?) -> String {
        if case .array(let items)? = v { return items.map { text($0) }.joined(separator: "\n") }
        return text(v)
    }

    static func humanize(_ key: String) -> String {
        key.replacingOccurrences(of: "_", with: " ").replacingOccurrences(of: "-", with: " ").lowercased().upperFirst
    }

    /// "Friday 2 October, 10:00–10:45" from the tool's local-time strings.
    static func when(_ start: String, _ end: String?) -> String {
        let allDay = start.count == 10
        guard let s = parseLocal(start) else { return end.map { "\(start) – \($0)" } ?? start }
        let day = s.formatted(.dateTime.weekday(.wide).day().month(.wide))
        if allDay {
            if let end, end != start, let e = parseLocal(end) {
                return "\(day) – \(e.formatted(.dateTime.weekday(.wide).day().month(.wide)))"
            }
            return "\(day) (all day)"
        }
        let e = end.flatMap(parseLocal) ?? s.addingTimeInterval(3600)
        let time = { (d: Date) in d.formatted(date: .omitted, time: .shortened) }
        if Calendar.current.isDate(e, inSameDayAs: s) { return "\(day), \(time(s))–\(time(e))" }
        return "\(day) \(time(s)) – \(e.formatted(date: .abbreviated, time: .shortened))"
    }

    private static func parseLocal(_ s: String) -> Date? {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = .current
        for format in ["yyyy-MM-dd'T'HH:mm:ss", "yyyy-MM-dd'T'HH:mm", "yyyy-MM-dd"] {
            f.dateFormat = format
            if let d = f.date(from: String(s.prefix(19))) { return d }
        }
        return ISO8601DateFormatter().date(from: s)
    }
}
