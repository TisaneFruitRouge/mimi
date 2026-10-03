import SwiftUI

/// One day, as calendar apps draw it: all-day events on top, then the hours, with events
/// as tinted blocks side by side where they overlap, reminders as small chips at their
/// time, and the red "now" line on today.
struct DayPage: View {
    let day: Int64
    @Environment(AppModel.self) private var model
    @Environment(CalendarRouter.self) private var router
    private let store = CalendarStore.shared
    private let math = CalendarMath()
    @State private var position = ScrollPosition(edge: .top)
    @State private var scrolled = false

    static let hour: CGFloat = 54
    static let gutter: CGFloat = 58
    static let topPad: CGFloat = 10
    /// A chip's height, in minutes of the grid.
    static var chipMinutes: Double { Double(22 / hour * 60) }

    private var span: CalendarStore.Span { CalendarView.weekSpan(of: day, math: math) }
    private var events: [CalendarEvent] { (store.events(in: span)?.events ?? []).filter { math.onDay($0, day) } }
    private var timed: [CalendarStore.Timed] {
        let next = math.addDays(day, 1)
        return store.timed(in: span).filter { $0.o.at >= day && $0.o.at < next }
    }

    var body: some View {
        let events = events
        let allDay = events.filter(\.allDay)
        VStack(spacing: 0) {
            if !allDay.isEmpty {
                AllDayStrip(events: allDay)
            }
            ScrollView(.vertical) {
                GeometryReader { geo in
                    grid(width: geo.size.width, events: events.filter { !$0.allDay })
                }
                .frame(height: 24 * Self.hour + Self.topPad * 2)
            }
            .scrollPosition($position)
            .scrollIndicators(.hidden)
            .refreshable { await reload() }
            .onAppear(perform: scrollToStart)
            .onChange(of: events.count) { _, _ in scrollToStart() }
        }
    }

    private func reload() async {
        guard let api = model.api else { return }
        store.eventsChanged()
        store.scheduleChanged()
        try? await store.loadItems(api)
        try? await store.ensureEvents(span, api: api)
        await store.ensureOccurrences(span, api: api)
    }

    /// Opens around now on today; other days from their first morning event.
    private func scrollToStart() {
        guard !scrolled else { return }
        let now = math.ms(Date())
        let hour: Double
        if math.sameDay(day, now) {
            hour = Double(now - day) / 3_600_000 - 1.5
        } else {
            let first = events.filter { !$0.allDay && $0.start >= day }.map { Double($0.start - day) / 3_600_000 }.min()
            guard first != nil || store.events(in: span) != nil else { return }
            hour = min(8, first ?? 8) - 0.5
        }
        scrolled = true
        position.scrollTo(y: max(0, hour) * Self.hour)
    }

    @ViewBuilder
    private func grid(width: CGFloat, events: [CalendarEvent]) -> some View {
        let area = width - Self.gutter - 8
        let placed = math.layoutDay(events, day: day)
        let timed = timed
        let chips = math.chipTops(timed.map(\.o.at), day: day, chipMinutes: Self.chipMinutes)
        ZStack(alignment: .topLeading) {
            // Double-tap an empty slot for a new event there.
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { point in
                    let minutes = max(0, Double(point.y - Self.topPad) / Self.hour * 60)
                    let slot = Int64((minutes / 30).rounded(.down) * 30)
                    router.sheet = .newEvent(start: day + slot * 60_000)
                }
            HourLines(hide: math.sameDay(day, math.ms(Date())) ? Double(math.ms(Date()) - day) / 60_000 : nil)
            ForEach(placed, id: \.event.id) { p in
                let top = CGFloat(p.top) / 60 * Self.hour + Self.topPad
                let height = CGFloat(p.bottom - p.top) / 60 * Self.hour - 2
                let w = area / CGFloat(p.columns)
                EventBlock(event: p.event, day: day, height: height, reminded: !store.reminders(for: p.event.id).isEmpty)
                    .frame(width: max(0, w - 2), height: max(0, height))
                    .offset(x: Self.gutter + CGFloat(p.column) * w, y: top + 1)
            }
            ForEach(Array(timed.enumerated()), id: \.element.id) { i, t in
                ReminderChip(timed: t)
                    .frame(maxWidth: area - 4, alignment: .trailing)
                    .offset(x: Self.gutter + 2, y: CGFloat(chips[i]) / 60 * Self.hour + Self.topPad)
            }
            if math.sameDay(day, math.ms(Date())) {
                NowLine(day: day, width: width)
            }
        }
        .frame(width: width, height: 24 * Self.hour + Self.topPad * 2, alignment: .topLeading)
    }
}

/// Hour marks and their hairlines.
private struct HourLines: View {
    /// Minutes of the day where the "now" time is written: hour labels near it give way.
    var hide: Double?

    var body: some View {
        ForEach(0..<25, id: \.self) { h in
            HStack(spacing: 8) {
                Text(label(h))
                    .font(.caption2.weight(.medium))
                    .monospacedDigit()
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .fixedSize()
                    .frame(width: DayPage.gutter - 10, alignment: .trailing)
                    .opacity(hide.map { abs($0 - Double(h * 60)) < 14 } == true ? 0 : 1)
                Rectangle()
                    .fill(Color(uiColor: .separator).opacity(0.6))
                    .frame(height: 0.5)
            }
            .frame(height: 14)
            .offset(y: CGFloat(h) * DayPage.hour + DayPage.topPad - 7)
        }
        .allowsHitTesting(false)
    }

    private func label(_ h: Int) -> String {
        guard h > 0 && h < 24 else { return h == 0 ? hourText(0) : "" }
        return hourText(h)
    }

    private func hourText(_ h: Int) -> String {
        let d = Calendar.current.date(bySettingHour: h, minute: 0, second: 0, of: Date()) ?? Date()
        let twelve = DateFormatter.dateFormat(fromTemplate: "j", options: 0, locale: .current)?.contains("a") == true
        return twelve ? d.formatted(.dateTime.hour()) : d.formatted(.dateTime.hour(.twoDigits(amPM: .omitted)).minute(.twoDigits))
    }
}

/// The red line at the current time, with its time in the gutter.
private struct NowLine: View {
    let day: Int64
    let width: CGFloat
    private let math = CalendarMath()

    var body: some View {
        TimelineView(.periodic(from: .now, by: 30)) { context in
            let now = math.ms(context.date)
            let y = CGFloat(now - day) / 3_600_000 * DayPage.hour + DayPage.topPad
            ZStack(alignment: .topLeading) {
                Text(math.clock(now))
                    .font(.caption2.weight(.semibold))
                    .monospacedDigit()
                    .foregroundStyle(Color.calendarRed)
                    .padding(.horizontal, 3)
                    .background(Color(uiColor: .systemBackground), in: .capsule)
                    .frame(width: DayPage.gutter - 4, alignment: .trailing)
                    .offset(y: y - 7)
                Circle()
                    .fill(Color.calendarRed)
                    .frame(width: 8, height: 8)
                    .offset(x: DayPage.gutter - 4, y: y - 4)
                Rectangle()
                    .fill(Color.calendarRed)
                    .frame(width: width - DayPage.gutter, height: 1.5)
                    .offset(x: DayPage.gutter, y: y - 0.75)
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// One timed event: a tint of its calendar's colour with a 3pt bar. Its text is the
/// calendar's, shown as written.
struct EventBlock: View {
    let event: CalendarEvent
    let day: Int64
    let height: CGFloat
    let reminded: Bool
    @Environment(CalendarRouter.self) private var router
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    var body: some View {
        let tint = EventTint(hex: store.color(of: event.calendarId))
        Button {
            router.sheet = .event(event)
        } label: {
            HStack(spacing: 0) {
                Rectangle().fill(tint.bar).frame(width: 3)
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 3) {
                        Text(verbatim: event.title)
                            .font(.caption.weight(.semibold))
                            .lineLimit(height > 44 ? 2 : 1)
                        if reminded {
                            Image(systemName: "bell.fill").font(.system(size: 8)).accessibilityLabel("Reminder set")
                        }
                    }
                    if height > 34 {
                        Text(verbatim: [event.start >= day ? math.clock(event.start) : nil, event.location].compactMap { $0 }.joined(separator: " · "))
                            .font(.caption2)
                            .opacity(0.8)
                            .lineLimit(1)
                    }
                }
                .foregroundStyle(tint.text)
                .padding(.horizontal, 5)
                .padding(.vertical, 3)
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(tint.fill)
            .clipShape(.rect(cornerRadius: 6))
            .contentShape(.rect(cornerRadius: 6))
        }
        .buttonStyle(CalendarPressStyle())
        .contextMenu { EventMenu(event: event) }
        .accessibilityLabel(Text(verbatim: "\(event.title), \(math.timeRange(event))"))
        .accessibilityIdentifier("calendar-event")
    }
}

/// A reminder or routine at its time: a small white capsule; what already went off is
/// quieter, and what didn't happen is struck through.
struct ReminderChip: View {
    let timed: CalendarStore.Timed
    @Environment(CalendarRouter.self) private var router
    private let math = CalendarMath()

    var body: some View {
        let past = timed.o.status != nil
        Button {
            router.sheet = .editItem(timed.item)
        } label: {
            HStack(spacing: 4) {
                TimedIcon(timed: timed)
                    .font(.system(size: 10, weight: .semibold))
                Text(verbatim: timed.item.title)
                    .font(.caption2.weight(.semibold))
                    .strikethrough(timed.notHappened)
                    .lineLimit(1)
                Text(verbatim: timed.clock(math))
                    .font(.caption2)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
            .foregroundStyle(past ? .secondary : .primary)
            .padding(.horizontal, 8)
            .frame(height: 22)
            .background(past ? Color.subtle : Color(uiColor: .systemBackground), in: .capsule)
            .shadow(color: .black.opacity(past ? 0 : 0.1), radius: 1.5, y: 0.5)
            .overlay(Capsule().strokeBorder(.black.opacity(past ? 0 : 0.06), lineWidth: 0.5))
        }
        .buttonStyle(CalendarPressStyle())
        .contextMenu { ScheduleItemMenu(item: timed.item, onEdit: { router.sheet = .editItem(timed.item) }, onOpenConversation: { router.conversation = $0 }) }
        .accessibilityLabel(Text(verbatim: timed.label(math)))
        .accessibilityIdentifier("calendar-reminder")
    }
}

/// The icon of a reminder at one of its times: a bell, the routine's sparkle, a clock
/// when snoozed.
struct TimedIcon: View {
    let timed: CalendarStore.Timed

    var body: some View {
        if timed.o.snoozed {
            Image(systemName: "clock").foregroundStyle(.secondary)
        } else if timed.item.kind == .routine {
            Image(systemName: "sparkles").foregroundStyle(timed.o.status == nil ? Color.limeDeep : .secondary)
        } else {
            Image(systemName: "bell.fill").foregroundStyle(.secondary)
        }
    }
}

extension CalendarStore.Timed {
    /// "7:00", or "9:00–23:30" for a day of something that repeats every few minutes.
    func clock(_ math: CalendarMath) -> String {
        o.count > 1 ? "\(math.clock(o.at))–\(math.clock(o.until))" : math.clock(o.at)
    }

    /// What happened to it, or will: "Sent", "Missed", "Snoozed"; nil for one to come.
    var state: String? {
        if o.snoozed { return "Snoozed" }
        return o.status.map(ScheduleWords.status)
    }

    /// Didn't happen: missed while the computer was off, or a routine run skipped.
    var notHappened: Bool { o.status == .missed || o.status == .skipped }

    /// "Water the plants · 18:00 · Sent".
    func label(_ math: CalendarMath) -> String {
        [item.title, clock(math), o.count > 1 ? item.description : nil, state].compactMap { $0 }.joined(separator: " · ")
    }
}

/// All-day events of a day, above the hours.
private struct AllDayStrip: View {
    let events: [CalendarEvent]
    @Environment(CalendarRouter.self) private var router
    private let store = CalendarStore.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            ForEach(events.prefix(3)) { e in
                let tint = EventTint(hex: store.color(of: e.calendarId))
                Button {
                    router.sheet = .event(e)
                } label: {
                    HStack(spacing: 0) {
                        Rectangle().fill(tint.bar).frame(width: 3)
                        Text(verbatim: e.title)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(tint.text)
                            .lineLimit(1)
                            .padding(.horizontal, 6)
                        Spacer(minLength: 0)
                    }
                    .frame(height: 24)
                    .background(tint.fill)
                    .clipShape(.rect(cornerRadius: 6))
                }
                .buttonStyle(CalendarPressStyle())
                .contextMenu { EventMenu(event: e) }
                .accessibilityLabel(Text(verbatim: "\(e.title), all day"))
                .accessibilityIdentifier("calendar-event")
            }
            if events.count > 3 {
                Text("\(events.count - 3) more").font(.caption2).foregroundStyle(.secondary).padding(.leading, 6)
            }
        }
        .padding(.leading, DayPage.gutter)
        .padding(.trailing, 8)
        .padding(.vertical, 6)
        .overlay(alignment: .topLeading) {
            Text("all-day")
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .frame(width: DayPage.gutter - 10, alignment: .trailing)
                .padding(.top, 11)
        }
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color(uiColor: .separator).opacity(0.6)).frame(height: 0.5)
        }
    }
}

/// A slight press, like the desktop's buttons.
struct CalendarPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .opacity(configuration.isPressed ? 0.85 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.7), value: configuration.isPressed)
    }
}
