import Foundation

/// The Mail panel's pure logic, the same rules as the desktop's `features/mail/*`: who a
/// conversation is with, replies and forwards, address lists, quoted history, and which
/// attachments are programs.
nonisolated enum MailText {
    // MARK: Showing conversations

    /// "Sam, Priya +2", or "To Sam" when the latest message is the user's.
    static func who(_ t: MailThread) -> String {
        let names = t.participants.map(\.shortName)
        if names.isEmpty { return "Just you" }
        let shown = names.prefix(2).joined(separator: ", ")
        let text = names.count > 2 ? "\(shown) +\(names.count - 2)" : shown
        return t.lastFromMe ? "To \(text)" : text
    }

    static func subject(_ s: String) -> String {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? "(no subject)" : t
    }

    /// A subject as a # tag: one line, as the computer's suggestions label it.
    static func label(_ subject: String) -> String {
        let s = subject.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        let one = s.isEmpty ? "(no subject)" : s
        return one.count > 60 ? String(one.prefix(60)) + "…" : one
    }

    /// The start of a message for the list, without the rules and dividers newsletters
    /// draw with characters ("─────", "=====").
    static func tidySnippet(_ s: String) -> String {
        let cleaned = s.replacingOccurrences(of: "([^\\p{L}\\p{N}\\s])\\1{2,}", with: " ", options: .regularExpression)
        return cleaned.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    /// "14:05", "Tue", "12 Mar".
    static func listDate(_ ms: Int64, now: Date = .now) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        let cal = Calendar.current
        if cal.isDate(date, inSameDayAs: now) { return date.formatted(date: .omitted, time: .shortened) }
        if cal.isDate(date, inSameDayAs: cal.date(byAdding: .day, value: -1, to: now) ?? now) { return "Yesterday" }
        if now.timeIntervalSince(date) < 6 * 86_400 { return date.formatted(.dateTime.weekday(.wide)) }
        if cal.isDate(date, equalTo: now, toGranularity: .year) { return date.formatted(.dateTime.day().month(.abbreviated)) }
        return date.formatted(.dateTime.day().month(.abbreviated).year())
    }

    /// "Tue 12 Mar, 14:05".
    static func longDate(_ ms: Int64) -> String {
        Date(timeIntervalSince1970: Double(ms) / 1000)
            .formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated).hour().minute())
    }

    // MARK: Addresses

    /// "sam@example.com, Bo <bo@example.net>; x@y" → each address as typed.
    static func splitAddresses(_ raw: String) -> [String] {
        raw.split(whereSeparator: { $0 == "," || $0 == ";" || $0 == "\n" })
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    static func joinAddresses(_ list: [String]) -> String { list.joined(separator: ", ") }

    /// The bare address in "Name <a@b>" or "a@b", lowercased.
    static func bare(_ address: String) -> String {
        let s = address.trimmingCharacters(in: .whitespaces)
        if let open = s.lastIndex(of: "<"), let close = s.lastIndex(of: ">"), open < close {
            return s[s.index(after: open)..<close].trimmingCharacters(in: .whitespaces).lowercased()
        }
        return s.lowercased()
    }

    /// Whether it looks like something mail can be sent to: one @ with text either side and
    /// a dot in the domain. The computer checks properly; this only catches typos early.
    static func looksLikeAddress(_ address: String) -> Bool {
        let a = bare(address)
        let parts = a.split(separator: "@", omittingEmptySubsequences: false)
        guard parts.count == 2, !parts[0].isEmpty, parts[1].contains("."),
              !parts[1].hasPrefix("."), !parts[1].hasSuffix("."), !a.contains(" ") else { return false }
        return true
    }

    // MARK: Replies and forwards

    /// A reply to whoever wrote last (other than the user), from the address the mail
    /// arrived at, with "Re:" once.
    static func reply(to d: MailThreadDetail) -> MailDraft {
        let lastOther = d.messages.last { !$0.fromMe }
        let to = lastOther.map { [$0.from.email] } ?? (d.messages.last?.to.map(\.email) ?? [])
        return MailDraft(
            connectionId: d.thread.connectionId,
            from: d.thread.receivedOn,
            to: to,
            subject: prefixed(d.thread.subject, "Re:", already: ["re", "aw", "sv", "antw", "ré", "réf"]),
            replyTo: d.thread.id
        )
    }

    /// Reply to everyone in the latest message, or `nil` when that's just the sender: the
    /// sender in To, everyone else (but the user) in Cc.
    static func replyAll(to d: MailThreadDetail, mine: Set<String>) -> MailDraft? {
        var base = reply(to: d)
        guard let last = d.messages.last(where: { !$0.fromMe }) ?? d.messages.last else { return nil }
        let to = Set(base.to.map { $0.lowercased() })
        var seen = Set<String>()
        let cc = (last.to + last.cc).map { $0.email.lowercased() }.filter { a in
            !mine.contains(a) && !to.contains(a) && seen.insert(a).inserted
        }
        guard !cc.isEmpty else { return nil }
        base.cc = cc
        return base
    }

    /// A new message forwarding `m` (the latest by default), quoted, its attachments along.
    static func forward(_ d: MailThreadDetail, message: MailMessage? = nil) -> MailDraft? {
        guard let m = message ?? d.messages.last else { return nil }
        var header = [
            "---------- Forwarded message ----------",
            "From: \(m.from.full)",
            "Date: \(longDate(m.date))",
            "Subject: \(d.thread.subject)",
        ]
        if !m.to.isEmpty { header.append("To: \(m.to.map(\.full).joined(separator: ", "))") }
        return MailDraft(
            connectionId: d.thread.connectionId,
            from: d.thread.receivedOn,
            subject: prefixed(d.thread.subject, "Fwd:", already: ["fwd", "fw", "tr", "wg"]),
            body: "\n\n\(header.joined(separator: "\n"))\n\n\(m.body)",
            forwardOf: m.attachments.isEmpty ? nil : m.id
        )
    }

    /// `subject` with `prefix` in front, unless it already starts with one of `already`.
    static func prefixed(_ subject: String, _ prefix: String, already: [String]) -> String {
        let s = subject.trimmingCharacters(in: .whitespaces)
        if let colon = s.firstIndex(of: ":") {
            let head = s[..<colon].trimmingCharacters(in: .whitespaces).lowercased()
            if already.contains(head) { return s }
        }
        return s.isEmpty ? prefix : "\(prefix) \(s)"
    }

    // MARK: Bodies

    /// A body split where its quoted history starts ("On … wrote:", "> …"), as the
    /// desktop folds it. The second part is `nil` when there's none (or it's all quoted).
    static func splitQuoted(_ body: String) -> (String, String?) {
        let lines = body.components(separatedBy: "\n")
        let at = lines.indices.first { i in
            let t = lines[i].trimmingCharacters(in: .whitespaces).lowercased()
            if t.hasPrefix(">") {
                return lines[i...].allSatisfy { l in
                    let x = l.trimmingCharacters(in: .whitespaces)
                    return x.isEmpty || x.hasPrefix(">")
                }
            }
            return (t.hasPrefix("on ") && t.hasSuffix("wrote:"))
                || (t.hasPrefix("le ") && (t.hasSuffix("a écrit :") || t.hasSuffix("a écrit:")))
                || (t.hasPrefix("am ") && t.hasSuffix("schrieb:"))
                || t.hasPrefix("-----original message-----")
        }
        guard let at, at > 0 else { return (body, nil) }
        let head = lines[..<at].joined(separator: "\n")
        let trimmed = head.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
        return (trimmed, lines[at...].joined(separator: "\n"))
    }

    // MARK: Attachments

    /// Programs, scripts, installers and launchers: opening one from an email could run
    /// it, so they're only saved. Files without an extension count too. The same list as
    /// the desktop's `attachments::is_program`.
    static func isProgram(_ name: String) -> Bool {
        let ext = (name as NSString).pathExtension.lowercased()
        if ext.isEmpty { return true }
        return programs.contains(ext)
    }

    static let programs: Set<String> = [
        "desktop", "sh", "bash", "zsh", "fish", "csh", "ksh", "run", "bin", "appimage", "flatpakref",
        "flatpak", "snap", "deb", "rpm", "pkg", "apk", "exe", "msi", "bat", "cmd", "com", "scr", "ps1",
        "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "cpl", "reg", "lnk", "jar", "py", "pyw",
        "pl", "rb", "php", "command", "tool", "app", "dmg", "iso", "img", "workflow", "scpt",
        "applescript", "action", "elf", "out", "so", "dll", "xpi", "crx", "service", "timer",
        // iPhone-specific: installers and configuration profiles.
        "ipa", "mobileconfig", "shortcut",
    ]

    /// A file name that stays inside the folder it's written to: no separators, no leading
    /// dots, no control characters, not too long.
    static func safeFileName(_ name: String) -> String {
        let cleaned = String(name.map { c -> Character in
            c == "/" || c == "\\" || c == ":" || c.unicodeScalars.contains { $0.properties.generalCategory == .control } ? "_" : c
        })
        var s = cleaned.trimmingCharacters(in: .whitespaces)
        while s.hasPrefix(".") { s.removeFirst() }
        s = s.trimmingCharacters(in: .whitespaces)
        if s.isEmpty { return "attachment" }
        if s.count <= 120 { return s }
        let ext = (s as NSString).pathExtension
        let stem = String(s.prefix(100))
        return ext.isEmpty || ext.count > 10 ? stem : "\(stem).\(ext)"
    }
}
