import SwiftUI

/// One person: how to reach them, what the assistant remembers, what's coming up with
/// them, recent email and the chats where they were mentioned.
struct PersonView: View {
    let id: UUID
    @Environment(AppModel.self) private var model
    @Environment(PeopleFlow.self) private var flow
    @Environment(\.openURL) private var openURL

    @State private var person: Person?
    @State private var gone = false
    @State private var notes: [MemoryNote] = []
    @State private var events: [PersonEvent] = []
    @State private var eventsLoaded = false
    @State private var mail: [PersonMailThread] = []
    @State private var chats: [PersonConversation] = []
    @State private var editingName = false
    @State private var addingHandle = false
    @State private var editingHandle: Handle?
    @State private var error: String?

    /// How far ahead "Coming up" looks.
    private static let aheadDays = 60

    var body: some View {
        Group {
            if let p = person {
                content(p)
            } else if gone {
                ContentUnavailableView("Not in your contacts anymore", systemImage: "person.crop.circle.badge.questionmark",
                                       description: Text("This person was deleted or merged into someone else."))
            } else {
                ProgressView()
            }
        }
        .navigationBarTitleDisplayMode(.inline)
        .task(id: model.revision("people_changed")) { await load() }
        .task(id: model.revision("memory_changed")) { await loadNotes() }
        .sheet(isPresented: $editingName) {
            if let p = person { NameSheet(person: p) { person = $0 } }
        }
        .sheet(isPresented: $addingHandle) {
            if let p = person {
                HandleSheet(title: "Add a way to reach \(p.firstName)", submitLabel: "Add") { h in
                    person = try await api().addHandle(p.id, h)
                }
            }
        }
        .sheet(item: $editingHandle) { h in
            HandleSheet(title: "Change \(h.channel.label.lowerFirst)", submitLabel: "Save",
                        initial: NewHandle(channel: h.channel, value: h.value, label: h.label)) { next in
                guard let p = person else { return }
                person = try await api().updateHandle(p.id, handle: h.id, next)
            }
        }
        .problemAlert($error)
    }

    @ViewBuilder
    private func content(_ p: Person) -> some View {
        List {
            Section {
                header(p)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
            }

            Section {
                ForEach(p.handles) { h in handleRow(h, of: p) }
                Button {
                    addingHandle = true
                } label: {
                    Label("Add a way to reach them", systemImage: "plus")
                }
            } header: {
                Text("How to reach them")
            } footer: {
                if p.handles.contains(where: { !$0.isOwn }) {
                    Text("Numbers and addresses from an address book are changed there, and \(model.assistantName) follows.")
                }
            }

            remembered(p)
            comingUp(p)
            recentMail(p)
            chatsSection(p)

            if p.isCombined {
                Section {
                    if p.manual {
                        LabeledContent(p.name, value: "Added by you")
                    }
                    ForEach(p.sources, id: \.self) { s in
                        HStack {
                            VStack(alignment: .leading, spacing: 1) {
                                Text(s.name)
                                Text(s.sourceId == nil ? "Added by you" : s.sourceName)
                                    .font(.footnote).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button("Not the same person") { Task { await split(p, s) } }
                                .font(.subheadline)
                                .buttonStyle(.borderless)
                        }
                    }
                } header: {
                    Text("Combined from")
                } footer: {
                    Text("Separate a card and it becomes its own person again.")
                }
            }
        }
        .listStyle(.insetGrouped)
        .listSectionSpacing(20)
        .refreshable { await load() }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Change name", systemImage: "pencil") { editingName = true }
                    Button("Add a way to reach them", systemImage: "plus") { addingHandle = true }
                    Button("Merge with…", systemImage: "arrow.triangle.merge") { flow.pickingFor = p.summary }
                    Divider()
                    Button("Delete contact…", systemImage: "trash", role: .destructive) { flow.deleting = p.summary }
                } label: {
                    Image(systemName: "ellipsis")
                }
                .accessibilityLabel("More for \(p.name)")
            }
        }
    }

    // MARK: Header

    private func header(_ p: Person) -> some View {
        VStack(spacing: 14) {
            PersonAvatar(id: p.id, name: p.name, size: 84)
            VStack(spacing: 3) {
                Text(p.name)
                    .font(.title2.weight(.semibold))
                    .multilineTextAlignment(.center)
                let line = p.nickname.map { "Also called \($0)" } ?? p.sourcesLine
                if !line.isEmpty {
                    Text(line).font(.subheadline).foregroundStyle(.secondary)
                }
            }
            HStack(spacing: 8) {
                QuickAction(title: "Ask", systemImage: "sparkles", prominent: true) {
                    model.ask(about: .person, id: p.id.mimiPath, label: p.name)
                }
                .accessibilityLabel("Ask \(model.assistantName) about \(p.firstName)")
                if let phone = p.handles.first(where: { $0.channel == .phone }) {
                    QuickAction(title: "Message", systemImage: "message.fill") { open("sms:", phone.value) }
                    QuickAction(title: "Call", systemImage: "phone.fill") { open("tel:", phone.value) }
                }
                if let email = p.handles.first(where: { $0.channel == .email }) {
                    QuickAction(title: "Email", systemImage: "envelope.fill") { open("mailto:", email.value) }
                }
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 4)
        .padding(.bottom, 6)
    }

    // MARK: Handles

    private func handleRow(_ h: Handle, of p: Person) -> some View {
        Menu {
            ForEach(actions(for: h), id: \.title) { a in
                Button(a.title, systemImage: a.symbol) { openURL(a.url) }
            }
            Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = h.value }
            if h.isOwn {
                Divider()
                Button("Change…", systemImage: "pencil") { editingHandle = h }
                Button("Remove", systemImage: "trash", role: .destructive) { Task { await removeHandle(h, of: p) } }
            } else {
                Divider()
                Text("From \(h.sourceName): change it in the address book")
            }
        } label: {
            HStack(spacing: 12) {
                ChannelGlyph(channel: h.channel, size: 15)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 1) {
                    Text(h.value).foregroundStyle(Color.primary)
                    Text(handleDetail(h)).font(.footnote).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .contentShape(.rect)
        }
        .swipeActions(edge: .trailing) {
            if h.isOwn {
                Button("Remove", systemImage: "trash") { Task { await removeHandle(h, of: p) } }.tint(.red)
                Button("Change", systemImage: "pencil") { editingHandle = h }.tint(.gray)
            }
        }
    }

    private struct HandleAction { var title: String; var symbol: String; var url: URL }

    /// What the phone can do with a number or an address.
    private func actions(for h: Handle) -> [HandleAction] {
        let digits = h.value.filter { $0.isNumber || $0 == "+" }
        func url(_ s: String) -> URL? { URL(string: s) }
        var list: [HandleAction] = []
        switch h.channel {
        case .phone:
            if let u = url("tel:\(digits)") { list.append(.init(title: "Call", symbol: "phone", url: u)) }
            if let u = url("sms:\(digits)") { list.append(.init(title: "Message", symbol: "message", url: u)) }
            if let u = url("facetime:\(digits)") { list.append(.init(title: "FaceTime", symbol: "video", url: u)) }
        case .email:
            if let u = url("mailto:\(h.value.mimiQueryEscaped.replacingOccurrences(of: "%40", with: "@"))") {
                list.append(.init(title: "Email", symbol: "envelope", url: u))
            }
        case .telegram:
            let user = h.value.trimmingCharacters(in: CharacterSet(charactersIn: "@ "))
            if !user.isEmpty, let u = url("https://t.me/\(user.mimiQueryEscaped)") {
                list.append(.init(title: "Open in Telegram", symbol: "paperplane", url: u))
            }
        case .whatsapp:
            let n = digits.filter(\.isNumber)
            if !n.isEmpty, let u = url("https://wa.me/\(n)") {
                list.append(.init(title: "Open in WhatsApp", symbol: "message", url: u))
            }
        default: break
        }
        return list
    }

    private func open(_ scheme: String, _ value: String) {
        let v = scheme == "mailto:" ? value : value.filter { $0.isNumber || $0 == "+" }
        if let u = URL(string: scheme + v) { openURL(u) }
    }

    // MARK: Sections

    @ViewBuilder
    private func remembered(_ p: Person) -> some View {
        Section {
            if notes.isEmpty {
                Text("Nothing noted yet. Tell \(model.assistantName) about \(p.firstName) in a chat and it'll remember.")
                    .font(.subheadline).foregroundStyle(.secondary)
            } else {
                ForEach(notes) { n in
                    NavigationLink(value: PeopleRoute.memoryNote(n.path)) {
                        HStack(spacing: 12) {
                            SettingsIcon(systemImage: "book.fill", tint: Color(hex: 0xF3E8FD), foreground: Color(hex: 0x8A3EC2))
                            VStack(alignment: .leading, spacing: 1) {
                                Text(n.title)
                                Text(MemoryFolders.forYou(firstLine(n.body)))
                                    .font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                            }
                        }
                    }
                }
            }
        } header: {
            Text("What \(model.assistantName) remembers")
        }
    }

    @ViewBuilder
    private func comingUp(_ p: Person) -> some View {
        let upcoming = Self.upcoming(events)
        Section("Coming up with them") {
            if !eventsLoaded {
                ProgressView().frame(maxWidth: .infinity)
            } else if upcoming.isEmpty {
                Text("Nothing in the next two months.").font(.subheadline).foregroundStyle(.secondary)
            } else {
                ForEach(upcoming.prefix(8)) { e in
                    HStack(spacing: 12) {
                        PersonDateTile(ms: e.start)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(e.title).lineLimit(1)
                            Text(eventLine(e)).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .contextMenu {
                        Button("Ask \(model.assistantName) about this", systemImage: "sparkles") {
                            model.ask(about: .event, id: e.id, label: e.title)
                        }
                        Button("Copy details", systemImage: "doc.on.doc") {
                            UIPasteboard.general.string = "\(e.title)\n\(eventLine(e))"
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func recentMail(_ p: Person) -> some View {
        if p.handles.contains(where: { $0.channel == .email }) && !mail.isEmpty {
            Section("Recent emails") {
                ForEach(mail.prefix(5)) { t in
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(t.subject.isEmpty ? "(no subject)" : t.subject)
                                .fontWeight(t.unread ? .semibold : .regular)
                                .lineLimit(1)
                            Text(t.summary ?? t.snippet)
                                .font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                        }
                        Spacer(minLength: 8)
                        Text(relativeDay(t.lastAt)).font(.footnote).foregroundStyle(.tertiary)
                    }
                    .contextMenu {
                        Button("Ask \(model.assistantName) about this", systemImage: "sparkles") {
                            model.ask(about: .mailThread, id: String(t.id), label: t.subject.isEmpty ? "(no subject)" : t.subject)
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func chatsSection(_ p: Person) -> some View {
        Section("Chats about them") {
            if chats.isEmpty {
                Text("Mention them with @ in a chat with \(model.assistantName) and it shows here.")
                    .font(.subheadline).foregroundStyle(.secondary)
            } else {
                ForEach(chats.prefix(8)) { c in
                    NavigationLink(value: PeopleRoute.conversation(c.id)) {
                        HStack {
                            Label(c.title.isEmpty ? "New chat" : c.title, systemImage: "bubble.left")
                                .lineLimit(1)
                            Spacer()
                            Text(relativeDay(c.updatedAt)).font(.footnote).foregroundStyle(.tertiary)
                        }
                    }
                }
            }
        }
    }

    // MARK: Loading

    private func api() throws -> MimiAPI {
        guard let api = model.api else { throw MimiError.unreachable }
        return api
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            let p = try await api.person(id)
            if p.id != id {
                // Merged into someone else: show them where they are now.
                flow.moved(from: id, to: p.id)
                return
            }
            person = p
            gone = false
        } catch MimiError.api("not_found", _) {
            gone = true
            return
        } catch {
            if person == nil { self.error = error.localizedDescription }
            return
        }
        let from = Int64(Calendar.current.startOfDay(for: .now).timeIntervalSince1970 * 1000)
        let to = from + Int64(Self.aheadDays) * 86_400_000
        async let ev = api.personEvents(id, from: from, to: to)
        async let ch = api.personConversations(id)
        async let ml = api.personMail(id)
        events = (try? await ev) ?? []
        eventsLoaded = true
        chats = (try? await ch) ?? []
        mail = (try? await ml) ?? []
        await loadNotes()
    }

    private func loadNotes() async {
        notes = (try? await model.api?.personMemory(id)) ?? []
    }

    private func removeHandle(_ h: Handle, of p: Person) async {
        do { person = try await api().removeHandle(p.id, handle: h.id) } catch { self.error = error.localizedDescription }
    }

    private func split(_ p: Person, _ s: PersonSource) async {
        do {
            let fresh = try await api().splitPerson(p.id, sourceId: s.sourceId, record: s.record)
            flow.notice = UndoNotice(text: "\(fresh.name) is now separate")
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }

    // MARK: Helpers

    /// A daily standup is one line, at its next time, not sixty.
    static func upcoming(_ events: [PersonEvent], now: Date = .now) -> [PersonEvent] {
        let nowMs = Int64(now.timeIntervalSince1970 * 1000)
        var seen = Set<String>()
        return events.sorted { $0.start < $1.start }.filter { e in
            let key = "\(e.calendarId)\n\(e.title)"
            guard e.end > nowMs, !seen.contains(key) else { return false }
            seen.insert(key)
            return true
        }
    }

    private func eventLine(_ e: PersonEvent) -> String {
        let start = Date(timeIntervalSince1970: Double(e.start) / 1000)
        let end = Date(timeIntervalSince1970: Double(e.end) / 1000)
        var parts = [start.formatted(.dateTime.weekday(.wide).day().month(.abbreviated))]
        parts.append(e.allDay ? "All day" : "\(start.formatted(date: .omitted, time: .shortened)) – \(end.formatted(date: .omitted, time: .shortened))")
        if let place = e.location, !place.isEmpty { parts.append(place) }
        return parts.joined(separator: " · ")
    }

    private func firstLine(_ body: String) -> String {
        body.split(separator: "\n")
            .map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "-*# ")) }
            .first { !$0.isEmpty } ?? ""
    }
}

/// A Contacts-style action under the name: an icon over a short word.
private struct QuickAction: View {
    let title: String
    let systemImage: String
    var prominent = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Image(systemName: systemImage).font(.system(size: 17, weight: .semibold))
                Text(title).font(.caption.weight(.medium))
            }
            .foregroundStyle(prominent ? Color.limeInk : Color.ink)
            .frame(maxWidth: .infinity, minHeight: 56)
            .background(prominent ? Color.lime : Color(.secondarySystemGroupedBackground),
                        in: .rect(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(.plain)
    }
}

/// A small calendar-page tile: weekday and day number.
private struct PersonDateTile: View {
    let ms: Int64

    var body: some View {
        let d = Date(timeIntervalSince1970: Double(ms) / 1000)
        VStack(spacing: 0) {
            Text(d.formatted(.dateTime.weekday(.abbreviated)).uppercased())
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(Color(hex: 0xFF3B30))
            Text(d.formatted(.dateTime.day()))
                .font(.system(size: 16, weight: .semibold))
                .monospacedDigit()
        }
        .frame(width: 38, height: 38)
        .background(Color(.tertiarySystemFill), in: .rect(cornerRadius: 10, style: .continuous))
        .accessibilityHidden(true)
    }
}
