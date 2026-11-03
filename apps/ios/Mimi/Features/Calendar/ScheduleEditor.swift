import SwiftUI

/// Adding a reminder or routine by hand, or changing one: every kind of rule the computer
/// knows, including "before an event" (which follows the event if it moves).
struct ScheduleEditor: View {
    let item: ScheduleItem?
    /// For a new one: what it starts with.
    let draft: ScheduleDraft?
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    private let store = CalendarStore.shared
    private let math = CalendarMath()

    @State private var kind: ScheduleKind = .reminder
    @State private var title = ""
    @State private var instruction = ""
    @State private var when = ScheduleDraft()
    @State private var busy = false
    @State private var error: String?
    @State private var confirmDelete = false
    @State private var upcoming: [CalendarEvent] = []
    @State private var ready = false
    @FocusState private var titleFocused: Bool

    private var routine: Bool { kind == .routine }
    private var canSave: Bool {
        !title.trimmingCharacters(in: .whitespaces).isEmpty
            && (!routine || !instruction.trimmingCharacters(in: .whitespaces).isEmpty)
            && when.schedule(math: math) != nil
    }

    var body: some View {
        NavigationStack {
            Form {
                if item == nil {
                    Section {
                        Picker("Kind", selection: $kind) {
                            Text("Reminder").tag(ScheduleKind.reminder)
                            Text("Routine").tag(ScheduleKind.routine)
                        }
                        .pickerStyle(.segmented)
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets())
                    }
                }

                Section {
                    TextField(routine ? "Name" : "Remind me to", text: $title, prompt: Text(routine ? "Morning briefing" : "Call Léa"))
                        .font(.body.weight(.medium))
                        .focused($titleFocused)
                        .accessibilityIdentifier("schedule-title")
                } header: {
                    Text(routine ? "Name" : "Remind me to")
                }

                if routine {
                    Section {
                        TextField("Instruction", text: $instruction, prompt: Text("Look at my calendar for today and tell me what's coming up."), axis: .vertical)
                            .lineLimit(3...8)
                    } header: {
                        Text("What your assistant should do")
                    } footer: {
                        Text("Your assistant does this on schedule and sends you the result. Anything it would send or change still waits for your OK.")
                    }
                }

                whenSection

                if let item {
                    Section {
                        Button("Delete \(item.kind == .routine ? "routine" : "reminder")", role: .destructive) { confirmDelete = true }
                    }
                }

                if let error {
                    Section {
                        Label(error, systemImage: "exclamationmark.circle")
                            .foregroundStyle(Color.danger)
                            .font(.subheadline)
                    }
                }
            }
            .navigationTitle(item == nil ? "New" : routine ? "Change routine" : "Change reminder")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        Task { await save() }
                    } label: {
                        if busy { ProgressView() } else { Text(item != nil ? "Save" : routine ? "Set up" : "Remind me") }
                    }
                    .disabled(!canSave || busy)
                    .accessibilityIdentifier("schedule-save")
                }
            }
            .confirmationDialog("Delete “\(item?.title ?? "")”?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive) {
                    if let item { ScheduleItemActions(model: model, store: store).delete(item) }
                    dismiss()
                }
            } message: {
                Text(item?.kind == .routine ? "It won't run again. Its past results stay in their conversation." : "You won't be reminded of it again.")
            }
        }
        .presentationDetents([.large])
        .onAppear(perform: prepare)
        .onChange(of: kind) { _, k in
            // A routine usually repeats: new ones start on "every day".
            if ready && k == .routine && when.repeatKind == .once {
                when.repeatKind = .daily
                when.date = math.date(ymd: nil, hhmm: "07:00")
            }
        }
        .task(id: when.repeatKind) {
            if when.repeatKind == .beforeEvent && upcoming.isEmpty { await loadUpcoming() }
        }
    }

    // MARK: When

    @ViewBuilder
    private var whenSection: some View {
        Section {
            Picker("Repeat", selection: $when.repeatKind) {
                ForEach(ScheduleDraft.Repeat.allCases) { r in
                    Text(r.label).tag(r)
                }
            }
            switch when.repeatKind {
            case .once:
                DatePicker("Date", selection: $when.date, in: Date()..., displayedComponents: [.date, .hourAndMinute])
            case .daily, .weekdays:
                DatePicker("Time", selection: $when.date, displayedComponents: [.hourAndMinute])
            case .weekly:
                WeekdayPicker(days: $when.days)
                DatePicker("Time", selection: $when.date, displayedComponents: [.hourAndMinute])
            case .monthly:
                Picker("Day of the month", selection: $when.dayOfMonth) {
                    ForEach(1...31, id: \.self) { Text("\($0)").tag($0) }
                }
                DatePicker("Time", selection: $when.date, displayedComponents: [.hourAndMinute])
            case .yearly:
                DatePicker("Date", selection: $when.date, displayedComponents: [.date])
                DatePicker("Time", selection: $when.date, displayedComponents: [.hourAndMinute])
            case .interval:
                Stepper(value: $when.every, in: 1...(when.unit == .hours ? 48 : 600), step: when.unit == .hours ? 1 : 5) {
                    Text("Every \(when.every) \(when.unit == .hours ? (when.every == 1 ? "hour" : "hours") : (when.every == 1 ? "minute" : "minutes"))")
                        .monospacedDigit()
                }
                Picker("Unit", selection: $when.unit) {
                    Text("Minutes").tag(ScheduleDraft.Unit.minutes)
                    Text("Hours").tag(ScheduleDraft.Unit.hours)
                }
                .pickerStyle(.segmented)
            case .beforeEvent:
                eventPicker
                Picker("When", selection: $when.minutesBefore) {
                    ForEach(Self.beforeChoices(including: when.minutesBefore), id: \.self) { m in
                        Text(ScheduleWords.beforeLabel(m)).tag(m)
                    }
                }
            }
        } header: {
            Text("When")
        } footer: {
            if let footer { Text(footer) }
        }
    }

    private var footer: String? {
        switch when.repeatKind {
        case .monthly where when.dayOfMonth > 28: "Shorter months use their last day."
        case .beforeEvent: "It follows the event if it moves."
        case .interval: "Counted from when you save it."
        default: nil
        }
    }

    @ViewBuilder
    private var eventPicker: some View {
        let picked = when.event
        Picker("Event", selection: Binding(
            get: { picked?.id ?? "" },
            set: { id in
                if let e = upcoming.first(where: { $0.id == id }) {
                    when.event = .init(id: e.id, title: e.title)
                    if title.trimmingCharacters(in: .whitespaces).isEmpty { title = e.title }
                }
            }
        )) {
            if picked == nil { Text("Choose…").tag("") }
            if let picked, !upcoming.contains(where: { $0.id == picked.id }) {
                Text(verbatim: picked.title).tag(picked.id)
            }
            ForEach(upcoming) { e in
                Text(verbatim: "\(e.title) · \(math.when(e.start, now: math.ms(Date())))").tag(e.id)
            }
        }
        .pickerStyle(.navigationLink)
    }

    static func beforeChoices(including current: Int) -> [Int] {
        Array(Set([0, 5, 10, 15, 30, 60, 120, 24 * 60, current])).sorted()
    }

    // MARK: Doing

    private func prepare() {
        guard !ready else { return }
        if let item {
            kind = item.kind
            title = item.title
            instruction = item.instruction ?? ""
            when = ScheduleDraft(schedule: item.schedule, math: math)
        } else {
            if let draft { when = draft }
            titleFocused = true
        }
        Task { ready = true }
    }

    private func loadUpcoming() async {
        guard let api = model.api else { return }
        let now = math.ms(Date())
        if let found = try? await api.events(from: now, to: math.addDays(now, 21)) {
            upcoming = found.events.filter { !$0.allDay && $0.start > now }.sorted { $0.start < $1.start }
        }
    }

    private func save() async {
        guard let api = model.api, let schedule = when.schedule(math: math) else { return }
        busy = true
        error = nil
        defer { busy = false }
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let cleanInstruction = routine ? instruction.trimmingCharacters(in: .whitespacesAndNewlines) : nil
        do {
            let saved: ScheduleItem
            if let item {
                saved = try await api.updateSchedule(item.id, ScheduleUpdate(title: cleanTitle, instruction: cleanInstruction, schedule: schedule))
            } else {
                saved = try await api.addSchedule(NewScheduleItem(kind: kind, title: cleanTitle, instruction: cleanInstruction, schedule: schedule))
            }
            store.upsert(saved)
            let next = saved.nextAt.map { math.when($0, now: math.ms(Date())).lowerFirst }
            if item != nil {
                store.say("Saved", detail: next.map { "Next: \($0)" })
            } else if routine {
                store.say("\(saved.title) is set up", detail: next.map { "First run \($0)" })
            } else {
                store.say("You'll be reminded", detail: next.map { $0.upperFirst })
            }
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// The days of the week as round toggles, in the phone's week order.
private struct WeekdayPicker: View {
    @Binding var days: Set<Weekday>

    private var ordered: [Weekday] {
        // Calendar's weekdays count from Sunday = 1; ours from Monday.
        let first = (Calendar.current.firstWeekday + 5) % 7
        return (0..<7).map { Weekday.allCases[(first + $0) % 7] }
    }

    var body: some View {
        HStack(spacing: 6) {
            ForEach(ordered) { d in
                let on = days.contains(d)
                Button {
                    if on { days.remove(d) } else { days.insert(d) }
                } label: {
                    Text(d.letter)
                        .font(.subheadline.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .frame(height: 36)
                        .foregroundStyle(on ? Color.white : .secondary)
                        .background(on ? Color.ink : Color.subtle, in: .circle)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(d.shortName)
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(.vertical, 4)
        .sensoryFeedback(.selection, trigger: days)
    }
}
