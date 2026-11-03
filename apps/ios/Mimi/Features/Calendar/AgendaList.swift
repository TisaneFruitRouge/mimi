import SwiftUI

/// What's coming, day by day, from the selected day: events and reminders together.
struct AgendaList: View {
    let span: CalendarStore.Span
    @Environment(AppModel.self) private var model
    @Environment(CalendarRouter.self) private var router
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    private struct DayGroup: Identifiable {
        var day: Int64
        var events: [CalendarEvent]
        var reminders: [CalendarStore.Timed]
        var id: Int64 { day }
    }

    private var groups: [DayGroup] {
        let events = store.events(in: span)?.events ?? []
        let timed = store.timed(in: span)
        var out: [DayGroup] = []
        var day = span.from
        while day < span.to {
            let next = math.addDays(day, 1)
            let first = day == span.from
            let todays = events
                .filter { math.onDay($0, day) && ($0.allDay || $0.start >= day || first) }
                .sorted { ($0.allDay ? 0 : 1, $0.start) < ($1.allDay ? 0 : 1, $1.start) }
            let reminders = timed.filter { $0.o.at >= day && $0.o.at < next }
            if !todays.isEmpty || !reminders.isEmpty { out.append(DayGroup(day: day, events: todays, reminders: reminders)) }
            day = next
        }
        return out
    }

    var body: some View {
        let groups = groups
        let now = math.ms(Date())
        List {
            ForEach(groups) { g in
                Section {
                    ForEach(g.reminders) { t in ReminderRow(timed: t) }
                    ForEach(g.events) { e in AgendaEventRow(event: e) }
                } header: {
                    Text(math.dayTitle(g.day, now: now))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(math.sameDay(g.day, now) ? Color.calendarRed : .secondary)
                        .textCase(nil)
                }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Color.canvas)
        .overlay {
            if store.events(in: span) == nil && store.calendarsLoaded && !store.calendars.isEmpty {
                ProgressView()
            } else if groups.isEmpty {
                ContentUnavailableView {
                    Label("Nothing planned", systemImage: "calendar")
                } description: {
                    Text("No events or reminders in the next \(CalendarView.agendaDays / 7) weeks.")
                }
            }
        }
        .refreshable {
            store.eventsChanged()
            store.scheduleChanged()
            if let api = model.api { try? await store.loadItems(api) }
        }
    }
}

private struct AgendaEventRow: View {
    let event: CalendarEvent
    @Environment(CalendarRouter.self) private var router
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    var body: some View {
        let e = event
        let tint = EventTint(hex: store.color(of: e.calendarId))
        Button {
            router.sheet = .event(e)
        } label: {
            HStack(spacing: 12) {
                VStack(alignment: .trailing, spacing: 1) {
                    Text(e.allDay ? "All day" : math.clock(e.start))
                        .font(.subheadline.weight(.medium))
                        .monospacedDigit()
                    if !e.allDay {
                        Text(math.clock(e.end)).font(.caption).monospacedDigit().foregroundStyle(.secondary)
                    }
                }
                .frame(width: 62, alignment: .trailing)
                RoundedRectangle(cornerRadius: 2).fill(tint.bar).frame(width: 4).frame(maxHeight: 40)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 5) {
                        Text(verbatim: e.title).font(.body.weight(.medium)).lineLimit(2)
                        if !store.reminders(for: e.id).isEmpty {
                            Image(systemName: "bell.fill").font(.caption2).foregroundStyle(.secondary).accessibilityLabel("Reminder set")
                        }
                    }
                    Text(verbatim: [e.location, e.calendar].compactMap { $0 }.joined(separator: " · "))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu { EventMenu(event: e) }
        .accessibilityIdentifier("calendar-event")
    }
}

private struct ReminderRow: View {
    let timed: CalendarStore.Timed
    @Environment(CalendarRouter.self) private var router
    private let math = CalendarMath()

    var body: some View {
        let t = timed
        Button {
            router.sheet = .editItem(t.item)
        } label: {
            HStack(spacing: 12) {
                Text(t.clock(math))
                    .font(.subheadline.weight(.medium))
                    .monospacedDigit()
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                    .frame(width: 62, alignment: .trailing)
                TimedIcon(timed: t).font(.subheadline).frame(width: 4)
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: t.item.title)
                        .font(.body)
                        .foregroundStyle(t.o.status != nil ? .secondary : .primary)
                        .strikethrough(t.notHappened)
                    if t.o.count > 1 {
                        Text(verbatim: t.item.description).font(.subheadline).foregroundStyle(.secondary)
                    }
                }
                Spacer(minLength: 0)
                if let state = t.state {
                    Text(state).font(.footnote).foregroundStyle(.tertiary)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            ScheduleItemMenu(item: t.item, onEdit: { router.sheet = .editItem(t.item) }, onOpenConversation: { router.conversation = $0 })
        }
        .accessibilityIdentifier("calendar-reminder")
    }
}
