import SwiftUI

/// Adding an event by hand (at `start`, if a slot was picked), or changing one. Guests
/// are saved without anyone being emailed; once saved, the sheet offers to send the
/// invitations.
struct EventEditor: View {
    let event: CalendarEvent?
    let start: Int64?
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    @State private var title = ""
    @State private var calendarId = ""
    @State private var allDay = false
    @State private var startDate = Date()
    @State private var endDate = Date()
    @State private var location = ""
    @State private var notes = ""
    @State private var guests: [GuestEntry] = []
    @State private var error: String?
    @State private var busy = false
    /// Once saved: what the guests could be sent.
    @State private var saved: (title: String, offers: [InvitationOffer])?
    @State private var ready = false
    @FocusState private var titleFocused: Bool

    private var editing: Bool { event != nil }
    private var calendar: CalendarInfo? { store.calendar(calendarId) }
    /// A Google calendar read by its address: the event opens in Google Calendar to save.
    private var opensGoogle: Bool { calendar.map { !$0.writable } ?? false }
    /// Guests only where they can be invited, and only on the user's own events.
    private var canInvite: Bool { (calendar?.guests ?? false) && (event?.mine ?? true) }

    var body: some View {
        NavigationStack {
            Group {
                if let saved {
                    SavedEvent(title: saved.title, offers: saved.offers) { dismiss() }
                } else {
                    form
                }
            }
            .navigationTitle(saved != nil ? "" : editing ? "Edit event" : "New event")
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.large])
        .interactiveDismissDisabled(busy)
        .onAppear(perform: prepare)
    }

    private var form: some View {
        Form {
            Section {
                TextField("Title", text: $title, prompt: Text("Dinner with Sam"))
                    .font(.body.weight(.medium))
                    .focused($titleFocused)
                    .submitLabel(.next)
                    .accessibilityIdentifier("event-title")
                TextField("Place", text: $location, prompt: Text("Place (optional)"))
            } footer: {
                if let event, event.repeats {
                    Text("Changes only this time. The rest of the series stays as it is.")
                }
            }

            if !editing {
                Section {
                    Picker("Calendar", selection: $calendarId) {
                        ForEach(store.calendars) { c in
                            HStack {
                                Image(systemName: "circle.fill").foregroundStyle(RGB(hex: c.color)?.color ?? .gray)
                                Text(verbatim: c.name)
                            }
                            .tag(c.id)
                        }
                    }
                    .pickerStyle(.navigationLink)
                } footer: {
                    Text(opensGoogle ? "Google Calendar opens with this filled in, for you to save." : "Saved straight into your calendar.")
                }
            }

            Section {
                Toggle("All day", isOn: $allDay.animation())
                DatePicker("Starts", selection: $startDate, displayedComponents: allDay ? [.date] : [.date, .hourAndMinute])
                DatePicker("Ends", selection: $endDate, in: startDate..., displayedComponents: allDay ? [.date] : [.date, .hourAndMinute])
            }
            .onChange(of: startDate) { old, new in
                // Moving the start keeps the length the same.
                guard ready else { return }
                let length = max(endDate.timeIntervalSince(old), allDay ? 0 : 15 * 60)
                endDate = new.addingTimeInterval(length)
            }

            if canInvite {
                GuestsSection(guests: $guests)
            } else if let event, !event.mine, let organizer = event.organizer {
                Section {
                } footer: {
                    Text(verbatim: "\(organizer.displayName) organizes this event, so only they can change its guests.")
                }
            }

            Section("Notes") {
                TextField("Notes", text: $notes, prompt: Text("Notes (optional)"), axis: .vertical)
                    .lineLimit(3...8)
            }

            if let error {
                Section {
                    Label(error, systemImage: "exclamationmark.circle")
                        .foregroundStyle(Color.danger)
                        .font(.subheadline)
                }
            }
        }
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button {
                    Task { await save() }
                } label: {
                    if busy { ProgressView() } else { Text(editing ? "Save" : opensGoogle ? "Open in Google" : "Add") }
                }
                .disabled(busy || title.trimmingCharacters(in: .whitespaces).isEmpty || calendarId.isEmpty)
                .accessibilityIdentifier("event-save")
            }
        }
    }

    private func prepare() {
        guard !ready else { return }
        let initial: Int64
        if let event {
            title = event.title
            calendarId = event.calendarId
            allDay = event.allDay
            location = event.location ?? ""
            notes = event.notes ?? ""
            guests = event.guests.map { GuestEntry(name: $0.personName ?? $0.name, email: $0.email) }
            initial = event.start
            // All-day events end at midnight after their last day.
            endDate = math.date(event.allDay ? math.addDays(event.end, -1) : event.end)
        } else {
            calendarId = (store.calendars.first(where: \.writable) ?? store.calendars.first)?.id ?? ""
            initial = start ?? nextHour()
            endDate = math.date(initial + 3_600_000)
            titleFocused = true
        }
        startDate = math.date(initial)
        // The start's onChange shouldn't move the end while filling in.
        Task { ready = true }
    }

    private func nextHour() -> Int64 {
        var c = math.calendar.dateComponents([.year, .month, .day, .hour], from: Date().addingTimeInterval(3600))
        c.minute = 0
        return math.ms(math.calendar.date(from: c) ?? Date())
    }

    private func save() async {
        guard let api = model.api else { return }
        let startMs = allDay ? math.startOfDay(math.ms(startDate)) : math.ms(startDate)
        let endMs = allDay ? math.addDays(math.startOfDay(math.ms(endDate)), 1) : math.ms(endDate)
        guard endMs > startMs else {
            error = "The event must end after it starts."
            return
        }
        busy = true
        error = nil
        defer { busy = false }
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            if let event {
                let changed = try await api.changeEvent(event.id, EventChange(
                    title: cleanTitle,
                    start: startMs,
                    end: endMs,
                    allDay: allDay,
                    location: location,
                    notes: notes,
                    guests: canInvite ? guests.map(\.text) : nil
                ))
                store.eventsChanged()
                finish(headline: "Saved", offers: changed.invitations, note: changed.note)
                return
            }
            let created = try await api.addEvent(NewCalendarEvent(
                calendarId: calendarId,
                title: cleanTitle,
                start: startMs,
                end: endMs,
                allDay: allDay,
                location: location.isEmpty ? nil : location,
                notes: notes.isEmpty ? nil : notes,
                guests: canInvite ? guests.map(\.text) : []
            ))
            if let link = created.openURL, link.hasPrefix("https://"), let url = URL(string: link) {
                openURL(url)
                store.say("Google Calendar is open with your event", detail: "Press Save there.")
                dismiss()
                return
            }
            store.eventsChanged()
            finish(headline: "Added to \(created.calendar)", offers: created.invitations, note: created.note)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func finish(headline: String, offers: [InvitationOffer], note: String?) {
        if offers.isEmpty {
            store.say(headline, detail: note)
            dismiss()
        } else {
            if let note { store.say(note) }
            withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { saved = (headline, offers) }
        }
    }
}

/// After saving an event with guests: nothing has been emailed; send the invitations now.
private struct SavedEvent: View {
    let title: String
    let offers: [InvitationOffer]
    let done: () -> Void

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    Image(systemName: "checkmark")
                        .font(.system(size: 18, weight: .bold))
                        .foregroundStyle(Color.privateTone)
                        .frame(width: 40, height: 40)
                        .background(Color.privateSoft, in: .circle)
                    Text(verbatim: title).font(.title2.weight(.bold))
                    Text("Nothing has been emailed to your guests yet.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 8, leading: 4, bottom: 8, trailing: 4))
            }
            Section {
                ForEach(offers) { InvitationRow(offer: $0) }
            }
        }
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("Done", action: done)
            }
        }
    }
}

/// Guests: a row each (swipe to remove), and a field that suggests people from People
/// by name or address, or takes an address typed out in full.
struct GuestsSection: View {
    @Binding var guests: [GuestEntry]
    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var suggestions: [GuestSuggestion] = []
    @FocusState private var focused: Bool

    private var query: String { text.trimmingCharacters(in: .whitespaces) }
    private var taken: Set<String> { Set(guests.map { $0.email.lowercased() }) }
    private var options: [GuestSuggestion] {
        query.isEmpty ? [] : suggestions.filter { !taken.contains($0.email.lowercased()) }
    }
    private var typed: String? {
        GuestEntry.isEmail(query) && !taken.contains(query.lowercased()) && !options.contains { $0.email.lowercased() == query.lowercased() }
            ? query : nil
    }

    var body: some View {
        Section {
            ForEach(guests) { g in
                VStack(alignment: .leading, spacing: 1) {
                    Text(verbatim: g.name ?? g.email)
                    if g.name != nil {
                        Text(verbatim: g.email).font(.footnote).foregroundStyle(.secondary)
                    }
                }
            }
            .onDelete { guests.remove(atOffsets: $0) }
            TextField("Guests", text: $text, prompt: Text("Add people by name or email"))
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.emailAddress)
                .focused($focused)
                .submitLabel(.done)
                .onSubmit(pickFirst)
                .accessibilityIdentifier("guest-field")
            ForEach(options) { s in
                Button {
                    add(GuestEntry(name: s.name, email: s.email))
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: "person.crop.circle.fill").font(.title2).foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(verbatim: s.name).foregroundStyle(.primary)
                            Text(verbatim: s.email).font(.footnote).foregroundStyle(.secondary)
                        }
                    }
                }
                .accessibilityLabel(Text(verbatim: "Invite \(s.name), \(s.email)"))
            }
            if let typed {
                Button {
                    add(GuestEntry(name: nil, email: typed))
                } label: {
                    Label {
                        Text(verbatim: "Invite \(typed)")
                    } icon: {
                        Image(systemName: "plus.circle.fill")
                    }
                }
            }
        } header: {
            Text("Guests")
        } footer: {
            Text("Nobody is emailed when you save. You can send the invitations next.")
        }
        .task(id: query) { await suggest() }
    }

    private func suggest() async {
        guard !query.isEmpty, let api = model.api else {
            suggestions = []
            return
        }
        try? await Task.sleep(for: .milliseconds(180))
        guard !Task.isCancelled else { return }
        if let found = try? await api.guestSuggestions(query) { suggestions = found }
    }

    private func pickFirst() {
        if let s = options.first { add(GuestEntry(name: s.name, email: s.email)) } else if let typed { add(GuestEntry(name: nil, email: typed)) }
    }

    private func add(_ g: GuestEntry) {
        if !taken.contains(g.email.lowercased()) {
            withAnimation { guests.append(GuestEntry(name: g.name, email: g.email.lowercased())) }
        }
        text = ""
        focused = true
    }
}
