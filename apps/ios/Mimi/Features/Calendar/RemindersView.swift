import SwiftUI

/// Reminders and routines: what's coming, with edit, pause, run now and delete (with
/// Undo). Reached from the Calendar's bell and from Settings › Reminders & notifications.
struct RemindersView: View {
    @Environment(AppModel.self) private var model
    @State private var store = CalendarStore.shared
    @State private var editing: EditTarget?
    @State private var conversation: UUID?
    @State private var showFinished = false
    @State private var error: String?
    private let math = CalendarMath()

    enum EditTarget: Identifiable {
        case new
        case item(ScheduleItem)
        var id: String {
            switch self {
            case .new: "new"
            case .item(let i): i.id.uuidString
            }
        }
    }

    var body: some View {
        let items = store.items.sorted { ($0.nextAt ?? .max, $0.createdAt) < ($1.nextAt ?? .max, $1.createdAt) }
        let upcoming = items.filter { !$0.finished }
        let done = items.filter(\.finished).sorted { ($0.lastAt ?? 0) > ($1.lastAt ?? 0) }
        List {
            if !upcoming.isEmpty {
                Section {
                    ForEach(upcoming) { row($0) }
                } footer: {
                    Text("Reminders reach you in the app, on your computer, and on Telegram if it's connected.")
                }
            }
            if !done.isEmpty {
                Section {
                    Button {
                        withAnimation { showFinished.toggle() }
                    } label: {
                        HStack {
                            Text("Finished").foregroundStyle(.primary)
                            Spacer()
                            Text("\(done.count)").foregroundStyle(.secondary)
                            Image(systemName: "chevron.right")
                                .font(.footnote.weight(.semibold))
                                .foregroundStyle(.tertiary)
                                .rotationEffect(.degrees(showFinished ? 90 : 0))
                        }
                    }
                    if showFinished {
                        ForEach(done) { row($0) }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .overlay {
            if store.itemsLoaded && upcoming.isEmpty && done.isEmpty {
                ContentUnavailableView {
                    Label("Nothing planned", systemImage: "bell")
                } description: {
                    Text("Ask in a chat, like “remind me tomorrow at 9 to call Léa”, or add one here.")
                } actions: {
                    Button("Add one") { editing = .new }
                        .buttonStyle(.bordered)
                        .buttonBorderShape(.capsule)
                }
            } else if !store.itemsLoaded {
                ProgressView()
            }
        }
        .overlay(alignment: .bottom) { CalendarToastView(store: store) }
        .navigationTitle("Reminders")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("New reminder or routine", systemImage: "plus") { editing = .new }
                    .disabled(model.phase != .online)
            }
        }
        .sheet(item: $editing) { target in
            switch target {
            case .new: ScheduleEditor(item: nil, draft: nil)
            case .item(let i): ScheduleEditor(item: i, draft: nil)
            }
        }
        .navigationDestination(item: $conversation) { ChatView(conversationId: $0) }
        .refreshable { await load() }
        .task(id: model.revision("schedule_changed")) { await load() }
        .alert("Something went wrong", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    private func row(_ i: ScheduleItem) -> some View {
        ScheduleItemRow(item: i)
            .contentShape(Rectangle())
            .onTapGesture { if !i.finished { editing = .item(i) } }
            .swipeActions(edge: .trailing) {
                Button("Delete", systemImage: "trash", role: .destructive) { delete(i) }
            }
            .swipeActions(edge: .leading) {
                if !i.finished {
                    Button(i.paused ? "Resume" : "Pause", systemImage: i.paused ? "play.fill" : "pause.fill") {
                        ScheduleItemActions(model: model, store: store).setPaused(i, !i.paused)
                    }
                    .tint(i.paused ? Color.privateTone : Color.gray)
                }
            }
            .contextMenu {
                ScheduleItemMenu(item: i, onEdit: { editing = .item(i) }, onOpenConversation: { conversation = $0 })
            }
            .accessibilityIdentifier("reminder-row")
    }

    private func delete(_ i: ScheduleItem) {
        ScheduleItemActions(model: model, store: store).delete(i)
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            try await store.loadItems(api)
        } catch is CancellationError {
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// A reminder or routine: its icon, name and when it's next.
struct ScheduleItemRow: View {
    let item: ScheduleItem
    private let math = CalendarMath()

    var body: some View {
        let i = item
        let routine = i.kind == .routine
        HStack(spacing: 12) {
            CalendarIconTile(
                systemName: routine ? "sparkles" : "bell.fill",
                tint: routine ? .limeDeep : .ink,
                fill: routine ? .limeSoft : Color.subtle
            )
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(verbatim: i.title).font(.body.weight(.medium)).lineLimit(2)
                    if i.paused { CalendarPill(text: "Paused") }
                }
                Text(verbatim: ScheduleWords.detail(i, math: math, now: math.ms(Date())))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
        }
        .opacity(i.finished ? 0.65 : 1)
        .padding(.vertical, 2)
    }
}

/// What a reminder's actions do, wherever it's shown.
struct ScheduleItemActions {
    let model: AppModel
    let store: CalendarStore

    func setPaused(_ i: ScheduleItem, _ paused: Bool) {
        guard let api = model.api else { return }
        Task {
            do {
                store.upsert(try await api.updateSchedule(i.id, ScheduleUpdate(paused: paused)))
                store.say(paused ? "“\(i.title)” is paused" : "“\(i.title)” is back on")
            } catch {
                fail(error)
            }
        }
    }

    func run(_ i: ScheduleItem) {
        guard let api = model.api else { return }
        Task {
            do {
                try await api.runRoutine(i.id)
                store.say("\(i.title) is running", detail: "The result goes to its conversation.")
            } catch {
                fail(error)
            }
        }
    }

    /// Deleted after a few seconds, with Undo meanwhile.
    func delete(_ i: ScheduleItem) {
        guard let api = model.api else { return }
        store.delete(i, api: api) { fail($0) }
    }

    /// A new chat about it. Event reminders tag their event.
    func ask(_ i: ScheduleItem) {
        let what = i.kind == .routine ? "my routine" : "my reminder"
        if case .beforeEvent(let id, let title, _) = i.schedule {
            model.askDraft = Draft(
                text: "About \(what) “\(i.title)” for @\(title) ",
                mentions: [Mention(kind: .event, id: id, label: title)]
            )
        } else {
            model.askDraft = Draft(text: "About \(what) “\(i.title)” (\(i.description.lowerFirst)): ", mentions: [])
        }
        model.tab = .chats
    }

    func fail(_ error: Error) {
        store.show(CalendarToast(message: error.localizedDescription, isError: true), seconds: 6)
    }
}

/// The long-press menu of a reminder or routine.
struct ScheduleItemMenu: View {
    let item: ScheduleItem
    let onEdit: () -> Void
    let onOpenConversation: (UUID) -> Void
    @Environment(AppModel.self) private var model

    var body: some View {
        let i = item
        let actions = ScheduleItemActions(model: model, store: CalendarStore.shared)
        let routine = i.kind == .routine
        if !i.finished {
            if case .unknown = i.schedule {} else {
                Button("Edit", systemImage: "pencil", action: onEdit)
            }
        }
        if routine && !i.finished {
            Button("Run now", systemImage: "play") { actions.run(i) }
        }
        if routine, let c = i.conversationId {
            Button("Show results", systemImage: "bubble.left") { onOpenConversation(c) }
        }
        if !i.finished {
            Button(i.paused ? "Resume" : "Pause", systemImage: i.paused ? "play.circle" : "pause.circle") {
                actions.setPaused(i, !i.paused)
            }
        }
        Button("Ask \(model.assistantName) about this", systemImage: "sparkles") { actions.ask(i) }
        Section {
            Button("Delete", systemImage: "trash", role: .destructive) { actions.delete(i) }
        }
    }
}
