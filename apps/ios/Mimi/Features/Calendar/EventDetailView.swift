import SwiftUI

/// One event: when, where, who (with their answers), its notes and reminders, and what
/// can be done with it. Everything shown comes from the calendar and may be written by
/// someone else, so it is plain text: no links, no formatting.
struct EventDetailView: View {
    let event: CalendarEvent
    @Environment(AppModel.self) private var model
    @Environment(CalendarRouter.self) private var router
    @Environment(\.dismiss) private var dismiss
    @State private var deleting: CalendarEvent?
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    var body: some View {
        let e = event
        let actions = EventActions(model: model, store: store)
        let reminders = store.reminders(for: e.id)
        let upcoming = e.start > math.ms(Date())
        let writable = store.writable(e)
        NavigationStack {
            List {
                Section {
                    header
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 4, bottom: 4, trailing: 4))

                Section {
                    Button {
                        dismiss()
                        actions.ask(e)
                    } label: {
                        Label("Ask \(model.assistantName) about this", systemImage: "sparkles")
                            .font(.body.weight(.semibold))
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .buttonBorderShape(.capsule)
                    .controlSize(.large)
                    .tint(Color.ink)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                }

                let people = e.people
                if !people.isEmpty {
                    Section(people.count == 1 ? "1 person" : "\(people.count) people") {
                        ForEach(people, id: \.person.email) { p in
                            if let id = p.person.personId {
                                // Someone in People: open their page.
                                Button {
                                    dismiss()
                                    model.show(person: id)
                                } label: {
                                    PersonRow(person: p.person, organizer: p.organizer)
                                }
                                .tint(.primary)
                            } else {
                                PersonRow(person: p.person, organizer: p.organizer)
                            }
                        }
                    }
                }

                if let notes = e.notes {
                    Section("Notes") {
                        Text(verbatim: notes)
                            .font(.callout)
                            .foregroundStyle(.primary.opacity(0.9))
                            .textSelection(.enabled)
                    }
                }

                Section {
                    ForEach(reminders) { r in
                        HStack {
                            Label {
                                Text(beforeText(r))
                            } icon: {
                                Image(systemName: "bell.fill").foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button("Remove this reminder", systemImage: "minus.circle.fill") {
                                remove(r)
                            }
                            .labelStyle(.iconOnly)
                            .foregroundStyle(Color.danger)
                            .buttonStyle(.plain)
                        }
                    }
                    if upcoming {
                        Menu {
                            Section("Follows the event if it moves") {
                                ForEach(ScheduleWords.before, id: \.minutes) { b in
                                    Button(b.label) { actions.remind(e, minutes: b.minutes) }
                                }
                            }
                        } label: {
                            Label("Remind me", systemImage: "bell.badge")
                        }
                    }
                } header: {
                    if !reminders.isEmpty || upcoming { Text("Reminders") }
                }
            }
            .listStyle(.insetGrouped)
            .listSectionSpacing(18)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close", systemImage: "xmark") { dismiss() }
                }
                if writable {
                    ToolbarItem(placement: .primaryAction) {
                        Button("Edit") { router.sheet = .editEvent(e) }
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        if writable && e.canInvite {
                            Button("Send invitations…", systemImage: "paperplane") { router.sheet = .invite(e) }
                        }
                        Button("Copy details", systemImage: "doc.on.doc") { actions.copy(e) }
                        Button("Hide “\(e.calendar)”", systemImage: "eye.slash") {
                            dismiss()
                            actions.hide(e)
                        }
                        if writable {
                            Section {
                                Button("Delete event…", systemImage: "trash", role: .destructive) { deleting = e }
                            }
                        }
                    } label: {
                        Label("More", systemImage: "ellipsis")
                    }
                }
            }
            .overlay(alignment: .bottom) { CalendarToastView(store: store) }
            .modifier(DeleteEventConfirmation(event: $deleting) { e, all in
                dismiss()
                actions.delete(e, all: all)
            })
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private var header: some View {
        let e = event
        let tint = EventTint(hex: store.color(of: e.calendarId))
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Circle().fill(tint.bar).frame(width: 9, height: 9)
                Text(verbatim: e.calendar)
                Spacer()
            }
            .font(.subheadline)
            .foregroundStyle(.secondary)
            Text(verbatim: e.title)
                .font(.title2.weight(.bold))
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
            VStack(alignment: .leading, spacing: 5) {
                Label {
                    Text(verbatim: math.whenLong(e, now: math.ms(Date())))
                } icon: {
                    Image(systemName: "clock")
                }
                if e.repeats {
                    Label("Repeats", systemImage: "repeat")
                }
                if let place = e.location {
                    Label {
                        Text(verbatim: place)
                    } icon: {
                        Image(systemName: "mappin.and.ellipse")
                    }
                }
            }
            .font(.callout)
            .labelStyle(DetailLabelStyle())
        }
    }

    private func beforeText(_ r: ScheduleItem) -> String {
        if case .beforeEvent(_, _, let minutes) = r.schedule { return ScheduleWords.beforeLabel(minutes) }
        return r.description
    }

    private func remove(_ r: ScheduleItem) {
        guard let api = model.api else { return }
        store.delete(r, api: api) { EventActions(model: model, store: store).fail($0) }
    }
}

/// An icon in grey, then the text, top-aligned for long places.
private struct DetailLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            configuration.icon.foregroundStyle(.secondary).frame(width: 18)
            configuration.title
        }
    }
}

/// Someone on the event: their name (People's, when known), address and answer.
private struct PersonRow: View {
    let person: EventPerson
    let organizer: Bool

    var body: some View {
        let name = person.displayName
        HStack(spacing: 12) {
            Text(initials(name))
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: 32, height: 32)
                .background(Color.subtle, in: .circle)
            VStack(alignment: .leading, spacing: 1) {
                Text(verbatim: name).font(.body).lineLimit(1)
                if name != person.email {
                    Text(verbatim: person.email).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 4)
            if organizer {
                Text("Organizer").font(.footnote).foregroundStyle(.secondary)
            } else if let answer = person.response?.label {
                Text(answer)
                    .font(.footnote)
                    .foregroundStyle(person.response == .accepted ? Color.privateTone : .secondary)
            }
        }
    }

    private func initials(_ name: String) -> String {
        let words = name.split(whereSeparator: { $0 == " " || $0 == "@" || $0 == "." }).prefix(2)
        return words.compactMap(\.first).map { String($0).uppercased() }.joined()
    }
}
