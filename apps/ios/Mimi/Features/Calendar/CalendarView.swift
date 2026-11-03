import SwiftUI

/// The Calendar tab: every calendar merged, with reminders and routines in it. A week
/// strip on top and the day below (swipe between days), or a list of what's coming.
struct CalendarView: View {
    enum Mode: String { case day, list }

    @Environment(AppModel.self) private var model
    @State private var store = CalendarStore.shared
    @State private var router = CalendarRouter()
    @AppStorage("mimi.calendar.view") private var mode: Mode = .day
    private let math = CalendarMath()
    /// Day 0 of the pager: today when the tab opened.
    @State private var anchor = CalendarMath().startOfDay(CalendarMath().ms(Date()))
    @State private var selected = CalendarMath().startOfDay(CalendarMath().ms(Date()))
    @State private var error: String?

    static let agendaDays = 28

    static func weekSpan(of day: Int64, math: CalendarMath) -> CalendarStore.Span {
        let start = math.startOfWeek(day)
        return .init(from: start, to: math.addDays(start, 7))
    }

    private var agendaSpan: CalendarStore.Span {
        .init(from: selected, to: math.addDays(selected, Self.agendaDays))
    }

    private var today: Int64 { math.startOfDay(math.ms(Date())) }

    /// "October", or "October 2027" in another year.
    private var title: String {
        let date = math.date(selected)
        let sameYear = math.calendar.component(.year, from: date) == math.calendar.component(.year, from: Date())
        return sameYear ? date.formatted(.dateTime.month(.wide)) : math.monthTitle(selected)
    }

    var body: some View {
        @Bindable var router = router
        NavigationStack {
            VStack(spacing: 0) {
                WeekStrip(selected: $selected, anchor: anchor)
                header
                Group {
                    switch mode {
                    case .day: DayPager(selected: $selected, anchor: anchor)
                    case .list: AgendaList(span: agendaSpan)
                    }
                }
                .frame(maxHeight: .infinity)
            }
            .background(Color(uiColor: .systemBackground))
            .navigationTitle(title)
            .toolbarTitleDisplayMode(.inlineLarge)
            .toolbar { toolbar }
            .overlay(alignment: .bottom) { CalendarToastView(store: store) }
            .navigationDestination(item: $router.conversation) { ChatView(conversationId: $0) }
            .navigationDestination(isPresented: $router.showReminders) { RemindersView() }
        }
        .environment(router)
        .sheet(item: $router.sheet) { sheet in
            sheetContent(sheet)
                .environment(router)
        }
        .modifier(DeleteEventConfirmation(event: $router.deleting) { e, all in EventActions(model: model, store: store).delete(e, all: all) })
        .task(id: model.paired?.endpointId) { store.bind(to: model.paired?.endpointId) }
        .task(id: model.revision("connections_changed")) { await loadCalendars() }
        .task(id: model.revision("schedule_changed")) { await loadItems() }
        .task(id: LoadKey(week: math.startOfWeek(selected), mode: mode, agendaFrom: selected, calendars: store.calendarsLoaded ? store.calendars.count : -1, events: store.eventGeneration, schedule: store.scheduleGeneration)) {
            await loadRanges()
        }
        .alert("Something went wrong", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    // MARK: Pieces

    @ViewBuilder
    private var header: some View {
        let unavailable = store.events(in: mode == .day ? Self.weekSpan(of: selected, math: math) : agendaSpan)?.unavailable ?? []
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(math.longDay(selected, now: math.ms(Date())))
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
                    .contentTransition(.numericText())
                Spacer()
                if !store.loading.isEmpty {
                    ProgressView().controlSize(.small)
                }
                if selected != today {
                    Button("Today") {
                        withAnimation(.spring(response: 0.4, dampingFraction: 0.86)) { selected = today }
                    }
                    .font(.subheadline.weight(.semibold))
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .controlSize(.small)
                    .tint(Color.calendarRed)
                    .transition(.opacity.combined(with: .scale(scale: 0.9)))
                }
            }
            if !unavailable.isEmpty {
                Label {
                    Text(verbatim: "Couldn't read \(unavailable.joined(separator: "; "))")
                } icon: {
                    Image(systemName: "exclamationmark.circle")
                }
                .font(.footnote)
                .foregroundStyle(Color.cloudTone)
                .lineLimit(2)
            }
            if store.calendarsLoaded && store.calendars.isEmpty {
                NoCalendars()
            }
        }
        .padding(.horizontal, 16)
        .padding(.bottom, 6)
        .animation(.default, value: selected)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            Button("Reminders and routines", systemImage: "bell") { router.showReminders = true }
        }
        ToolbarItem(placement: .topBarTrailing) {
            Menu {
                Picker("View", selection: $mode) {
                    Label("Day", systemImage: "calendar.day.timeline.left").tag(Mode.day)
                    Label("List", systemImage: "list.bullet").tag(Mode.list)
                }
                Section {
                    Button("Calendars…", systemImage: "eye") { router.sheet = .calendars }
                        .disabled(store.calendars.isEmpty)
                }
            } label: {
                Label("View options", systemImage: "ellipsis")
            }
        }
        ToolbarSpacer(.fixed, placement: .topBarTrailing)
        ToolbarItem(placement: .topBarTrailing) {
            Menu {
                Button("New event", systemImage: "calendar.badge.plus") { router.sheet = .newEvent(start: nil) }
                    .disabled(store.calendars.isEmpty)
                Button("New reminder or routine", systemImage: "bell.badge") { router.sheet = .newItem(nil) }
            } label: {
                Label("Add", systemImage: "plus")
            }
            .disabled(model.phase != .online)
        }
    }

    @ViewBuilder
    private func sheetContent(_ sheet: CalendarRouter.Sheet) -> some View {
        switch sheet {
        case .event(let e):
            EventDetailView(event: e)
        case .newEvent(let start):
            EventEditor(event: nil, start: start)
        case .editEvent(let e):
            EventEditor(event: e, start: nil)
        case .invite(let e):
            SendInvitationsSheet(event: e)
        case .newItem(let draft):
            ScheduleEditor(item: nil, draft: draft)
        case .editItem(let item):
            ScheduleEditor(item: item, draft: nil)
        case .calendars:
            CalendarsSheet()
        }
    }

    // MARK: Loading

    private struct LoadKey: Hashable {
        var week: Int64
        var mode: Mode
        var agendaFrom: Int64
        var calendars: Int
        var events: Int
        var schedule: Int
    }

    private func loadCalendars() async {
        guard let api = model.api else { return }
        do {
            try await store.loadCalendars(api)
            store.eventsChanged()
        } catch {
            handle(error)
        }
    }

    private func loadItems() async {
        guard let api = model.api else { return }
        do {
            try await store.loadItems(api)
            store.scheduleChanged()
        } catch {
            handle(error)
        }
    }

    /// The week on screen and its neighbours (so swiping is instant), or the list's weeks.
    private func loadRanges() async {
        guard let api = model.api, store.calendarsLoaded else { return }
        let spans: [CalendarStore.Span]
        switch mode {
        case .day:
            let week = Self.weekSpan(of: selected, math: math)
            spans = [week, Self.weekSpan(of: math.addDays(week.from, -7), math: math), Self.weekSpan(of: math.addDays(week.from, 7), math: math)]
        case .list:
            spans = [agendaSpan]
        }
        do {
            for span in spans {
                async let occurrences: Void = store.ensureOccurrences(span, api: api)
                try await store.ensureEvents(span, api: api)
                await occurrences
            }
        } catch {
            handle(error)
        }
    }

    private func handle(_ error: Error) {
        if error is CancellationError { return }
        if let e = error as? MimiError, e == .unreachable || e == .unpaired {
            model.handle(error)
            return
        }
        self.error = error.localizedDescription
    }
}

/// The week of the selected day: tap a day, swipe for the week before or after.
struct WeekStrip: View {
    @Binding var selected: Int64
    let anchor: Int64
    private let store = CalendarStore.shared
    private let math = CalendarMath()
    @State private var week: Int?

    private static let range = -260...260

    private func weekIndex(of day: Int64) -> Int {
        math.daysBetween(math.startOfWeek(anchor), math.startOfWeek(day)) / 7
    }

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach(Self.range, id: \.self) { i in
                    WeekRow(start: math.addDays(math.startOfWeek(anchor), i * 7), selected: $selected)
                        .containerRelativeFrame(.horizontal)
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.paging)
        .scrollIndicators(.hidden)
        .scrollPosition(id: $week)
        .frame(height: 64)
        .onAppear { week = weekIndex(of: selected) }
        .onChange(of: week) { old, new in
            guard let old, let new, new != weekIndex(of: selected) else { return }
            // Swiping the strip keeps the weekday, as Calendar does.
            selected = math.addDays(selected, (new - old) * 7)
        }
        .onChange(of: selected) { _, day in
            let w = weekIndex(of: day)
            if w != week { withAnimation(.spring(response: 0.4, dampingFraction: 0.9)) { week = w } }
        }
        .sensoryFeedback(.selection, trigger: selected)
    }
}

private struct WeekRow: View {
    let start: Int64
    @Binding var selected: Int64
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    var body: some View {
        let span = CalendarStore.Span(from: start, to: math.addDays(start, 7))
        let events = store.events(in: span)?.events ?? []
        let today = math.startOfDay(math.ms(Date()))
        HStack(spacing: 0) {
            ForEach(0..<7, id: \.self) { i in
                let day = math.addDays(start, i)
                let date = math.date(day)
                let isSelected = day == selected
                let isToday = day == today
                let weekend = math.calendar.isDateInWeekend(date)
                Button {
                    withAnimation(.spring(response: 0.4, dampingFraction: 0.86)) { selected = day }
                } label: {
                    VStack(spacing: 4) {
                        Text(date.formatted(.dateTime.weekday(.narrow)))
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(isToday ? Color.calendarRed : weekend ? Color.secondary.opacity(0.7) : Color.secondary)
                        Text(date.formatted(.dateTime.day()))
                            .font(.title3.weight(isSelected || isToday ? .semibold : .regular))
                            .monospacedDigit()
                            .foregroundStyle(isSelected ? Color.white : isToday ? Color.calendarRed : Color.primary)
                            .frame(width: 38, height: 38)
                            .background {
                                if isSelected {
                                    Circle().fill(isToday ? Color.calendarRed : Color.ink)
                                }
                            }
                        Circle()
                            .fill(Color.secondary.opacity(0.45))
                            .frame(width: 4, height: 4)
                            .opacity(events.contains { math.onDay($0, day) } ? 1 : 0)
                    }
                    .frame(maxWidth: .infinity)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(math.longDay(day, now: today))
                .accessibilityAddTraits(isSelected ? .isSelected : [])
            }
        }
        .padding(.horizontal, 8)
    }
}

/// Days side by side: swipe for the day before or after.
private struct DayPager: View {
    @Binding var selected: Int64
    let anchor: Int64
    private let math = CalendarMath()
    @State private var index: Int?

    private static let range = -1500...1500

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach(Self.range, id: \.self) { i in
                    DayPage(day: math.addDays(anchor, i))
                        .containerRelativeFrame(.horizontal)
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.paging)
        .scrollIndicators(.hidden)
        .scrollPosition(id: $index)
        .overlay(alignment: .top) {
            Rectangle().fill(Color(uiColor: .separator).opacity(0.6)).frame(height: 0.5)
        }
        .onAppear { index = math.daysBetween(anchor, selected) }
        .onChange(of: index) { _, i in
            guard let i else { return }
            let day = math.addDays(anchor, i)
            if day != selected { selected = day }
        }
        .onChange(of: selected) { _, day in
            let i = math.daysBetween(anchor, day)
            if i != index { withAnimation(.spring(response: 0.42, dampingFraction: 0.9)) { index = i } }
        }
    }
}

/// When nothing is connected: where to connect a calendar.
private struct NoCalendars: View {
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            CalendarIconTile(systemName: "calendar", tint: .networkTone, fill: .networkSoft, size: 34)
            VStack(alignment: .leading, spacing: 2) {
                Text("Connect a calendar").font(.subheadline.weight(.semibold))
                Text("Google, iCloud, Fastmail or Nextcloud: connect it in Mimi on your computer, in Settings › Connections. Your events then show here too.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.canvas, in: .rect(cornerRadius: 14))
        .padding(.top, 4)
    }
}

/// Which calendars show here: a choice on this phone only.
private struct CalendarsSheet: View {
    @Environment(\.dismiss) private var dismiss
    private let store = CalendarStore.shared

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(store.calendars) { c in
                        let on = !store.hidden.contains(c.id)
                        Button {
                            store.setHidden(c.id, on)
                        } label: {
                            HStack(spacing: 12) {
                                let color = RGB(hex: c.color)?.color ?? .gray
                                Image(systemName: on ? "checkmark.circle.fill" : "circle")
                                    .font(.title3)
                                    .foregroundStyle(color)
                                Text(verbatim: c.name).foregroundStyle(.primary)
                                Spacer()
                                if c.google { Text("Google").font(.footnote).foregroundStyle(.secondary) }
                                if !c.writable { Image(systemName: "lock").font(.footnote).foregroundStyle(.tertiary) }
                            }
                        }
                        .accessibilityAddTraits(on ? .isSelected : [])
                    }
                } footer: {
                    Text("Hidden calendars are only hidden on this phone. Your assistant still sees them.")
                }
            }
            .navigationTitle("Calendars")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done", systemImage: "checkmark") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}
