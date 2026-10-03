import Foundation
import Observation

/// What the Calendar, the reminder list and Settings › Reminders & notifications show:
/// calendars, events by range, reminders and routines with their times, and what went
/// off lately. Shared by those screens (one copy, so a change in one shows in all), kept
/// fresh by the computer's events (`schedule_changed`, `connections_changed`) and by
/// the user's own changes.
@Observable
final class CalendarStore {
    static let shared = CalendarStore()

    nonisolated struct Span: Hashable, Sendable {
        var from: Int64
        var to: Int64
    }

    private struct Loaded<T> {
        var value: T
        var generation: Int
    }

    private(set) var calendars: [CalendarInfo] = []
    private(set) var calendarsLoaded = false
    private var events: [Span: Loaded<CalendarEvents>] = [:]
    private var occurrences: [Span: Loaded<[ScheduleOccurrence]>] = [:]
    /// An older computer without `/schedule/occurrences`: only each item's next time.
    private(set) var occurrencesUnsupported = false
    private(set) var allItems: [ScheduleItem] = []
    private(set) var itemsLoaded = false
    private(set) var deliveries: [Delivery] = []
    private(set) var telegramConnected = false
    /// Calendars this viewer hid. A convenience on this phone only.
    private(set) var hidden: Set<String>
    /// Reminders deleted a moment ago, still undoable: hidden here, deleted on the
    /// computer once the Undo has gone.
    private var pendingDeletes: [UUID: Task<Void, Never>] = [:]
    private(set) var loading: Set<Span> = []
    /// Bumped by changes, so ranges already shown are read again (keeping what's on
    /// screen until the new copy is in).
    private(set) var eventGeneration = 0
    private(set) var scheduleGeneration = 0
    /// The computer this copy is from.
    private var computer: String?
    /// Something to tell the user at the bottom of the screen.
    var toast: CalendarToast?
    private var toastTimer: Task<Void, Never>?

    private static let hiddenKey = "mimi.calendar.hidden"

    init() {
        hidden = Set(UserDefaults.standard.stringArray(forKey: Self.hiddenKey) ?? [])
    }

    /// Keeps what's held only while it's the same computer.
    func bind(to computer: String?) {
        guard computer != self.computer else { return }
        self.computer = computer
        reset()
    }

    /// Forgets everything (another computer, or none).
    func reset() {
        calendars = []
        calendarsLoaded = false
        events = [:]
        occurrences = [:]
        allItems = []
        itemsLoaded = false
        deliveries = []
        loading = []
    }

    // MARK: Reading

    func color(of calendarId: String) -> String? { calendars.first { $0.id == calendarId }?.color }
    func calendar(_ id: String) -> CalendarInfo? { calendars.first { $0.id == id } }
    func writable(_ e: CalendarEvent) -> Bool { calendar(e.calendarId)?.writable ?? false }

    /// Events in a range, without hidden calendars. Nil until first read.
    func events(in span: Span) -> CalendarEvents? {
        guard var found = events[span]?.value else { return nil }
        found.events.removeAll { hidden.contains($0.calendarId) }
        return found
    }

    /// Reminders and routines, without those being deleted.
    var items: [ScheduleItem] { allItems.filter { pendingDeletes[$0.id] == nil } }

    func item(_ id: UUID) -> ScheduleItem? { items.first { $0.id == id } }

    /// Reminders tied to this occurrence of an event.
    func reminders(for eventId: String) -> [ScheduleItem] {
        items.filter {
            if case .beforeEvent(let id, _, _) = $0.schedule { id == eventId && $0.ended == nil } else { false }
        }
    }

    /// A reminder or routine at one of its times.
    struct Timed: Identifiable, Hashable {
        var item: ScheduleItem
        var o: ScheduleOccurrence
        var id: String { "\(item.id):\(o.at):\(o.snoozed)" }
    }

    /// Reminders at times of their own in a range; event-relative ones show as a bell on
    /// their event instead.
    func timed(in span: Span) -> [Timed] {
        let own = { (i: ScheduleItem) in !i.schedule.isBeforeEvent }
        if occurrencesUnsupported {
            return items
                .filter { own($0) && !$0.finished && !$0.paused && $0.nextAt.map { $0 >= span.from && $0 < span.to } == true }
                .map { Timed(item: $0, o: ScheduleOccurrence(itemId: $0.id, at: $0.nextAt!)) }
        }
        let byId = Dictionary(items.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        return (occurrences[span]?.value ?? []).compactMap { o in
            guard let item = byId[o.itemId], own(item) else { return nil }
            return Timed(item: item, o: o)
        }
        .sorted { $0.o.at < $1.o.at }
    }

    // MARK: Loading

    func loadCalendars(_ api: MimiAPI) async throws {
        calendars = try await api.calendars()
        calendarsLoaded = true
    }

    /// Reads a range unless the copy held is current.
    func ensureEvents(_ span: Span, api: MimiAPI) async throws {
        if let have = events[span], have.generation == eventGeneration { return }
        guard !calendars.isEmpty else {
            events[span] = Loaded(value: CalendarEvents(), generation: eventGeneration)
            return
        }
        let generation = eventGeneration
        loading.insert(span)
        defer { loading.remove(span) }
        let found = try await api.events(from: span.from, to: span.to)
        events[span] = Loaded(value: found, generation: generation)
    }

    func ensureOccurrences(_ span: Span, api: MimiAPI) async {
        if occurrencesUnsupported { return }
        if let have = occurrences[span], have.generation == scheduleGeneration { return }
        let generation = scheduleGeneration
        do {
            occurrences[span] = Loaded(value: try await api.scheduleOccurrences(from: span.from, to: span.to), generation: generation)
        } catch MimiError.outdated {
            occurrencesUnsupported = true
        } catch {}
    }

    func loadItems(_ api: MimiAPI) async throws {
        allItems = try await api.scheduleItems()
        itemsLoaded = true
    }

    func loadDeliveries(_ api: MimiAPI) async throws {
        deliveries = try await api.deliveries()
    }

    func loadConnections(_ api: MimiAPI) async {
        if let list = try? await api.connectionSummaries() {
            telegramConnected = list.contains { $0.integration == "telegram" && $0.status == "ok" }
        }
    }

    /// Calendars or their events changed (a connection, or the user's own edit): ranges
    /// on screen are read again.
    func eventsChanged() { eventGeneration += 1 }

    /// Reminders changed: their times are read again.
    func scheduleChanged() { scheduleGeneration += 1 }

    // MARK: Hiding calendars

    func setHidden(_ id: String, _ hide: Bool) {
        if hide { hidden.insert(id) } else { hidden.remove(id) }
        UserDefaults.standard.set(Array(hidden), forKey: Self.hiddenKey)
    }

    // MARK: Changing reminders

    /// Applies a reminder the computer just returned, before its event catches up.
    func upsert(_ item: ScheduleItem) {
        if let i = allItems.firstIndex(where: { $0.id == item.id }) { allItems[i] = item } else { allItems.append(item) }
        scheduleChanged()
    }

    /// Deletes a reminder after a few seconds, unless the user takes it back.
    func delete(_ item: ScheduleItem, api: MimiAPI, onError: @escaping (Error) -> Void) {
        pendingDeletes[item.id]?.cancel()
        pendingDeletes[item.id] = Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled else { return }
            await self?.commitDelete(item.id, api: api, onError: onError)
        }
        show(CalendarToast(
            message: "“\(item.title)” deleted",
            action: CalendarToast.Action(label: "Undo") { [weak self] in self?.undoDelete(item.id) }
        ), seconds: 5)
    }

    private func commitDelete(_ id: UUID, api: MimiAPI, onError: (Error) -> Void) async {
        do {
            try await api.deleteSchedule(id)
            allItems.removeAll { $0.id == id }
            scheduleChanged()
        } catch {
            onError(error)
        }
        pendingDeletes[id] = nil
    }

    func undoDelete(_ id: UUID) {
        pendingDeletes[id]?.cancel()
        pendingDeletes[id] = nil
        toast = nil
    }

    // MARK: Toasts

    func show(_ toast: CalendarToast, seconds: Double = 4) {
        self.toast = toast
        toastTimer?.cancel()
        let id = toast.id
        toastTimer = Task { [weak self] in
            try? await Task.sleep(for: .seconds(seconds))
            guard !Task.isCancelled, self?.toast?.id == id else { return }
            self?.toast = nil
        }
    }

    func say(_ message: String, detail: String? = nil) {
        show(CalendarToast(message: message, detail: detail))
    }
}

/// A short message at the bottom of the screen, with at most one action.
struct CalendarToast: Identifiable, Equatable {
    struct Action {
        var label: String
        var run: () -> Void
    }

    let id = UUID()
    var message: String
    var detail: String?
    var action: Action?
    var isError = false

    static func == (a: CalendarToast, b: CalendarToast) -> Bool { a.id == b.id }
}
