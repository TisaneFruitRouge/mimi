import SwiftUI
import UIKit

/// What can be done with an event from anywhere it's shown (its context menu, its
/// sheet): be reminded, ask, copy, edit, invite, delete, hide its calendar.
struct EventActions {
    let model: AppModel
    let store: CalendarStore
    let math = CalendarMath()

    /// Adds a reminder `minutes` before the event; it follows the event if it moves.
    func remind(_ e: CalendarEvent, minutes: Int) {
        guard let api = model.api else { return }
        Task {
            do {
                let item = try await api.addSchedule(NewScheduleItem(
                    kind: .reminder,
                    title: e.title,
                    instruction: nil,
                    schedule: .beforeEvent(eventId: e.id, eventTitle: e.title, minutesBefore: minutes)
                ))
                store.upsert(item)
                store.say("You'll be reminded \(ScheduleWords.beforeLabel(minutes).lowerFirst)")
            } catch {
                fail(error)
            }
        }
    }

    func removeReminders(of e: CalendarEvent) {
        guard let api = model.api else { return }
        let reminders = store.reminders(for: e.id)
        Task {
            do {
                for r in reminders { try await api.deleteSchedule(r.id) }
                try await store.loadItems(api)
                store.scheduleChanged()
                store.say(reminders.count > 1 ? "Reminders removed" : "Reminder removed")
            } catch {
                fail(error)
            }
        }
    }

    func ask(_ e: CalendarEvent) {
        model.ask(about: .event, id: e.id, label: e.title)
    }

    func copy(_ e: CalendarEvent) {
        UIPasteboard.general.string = math.eventText(e, now: math.ms(Date()))
        store.say("Copied")
    }

    func hide(_ e: CalendarEvent) {
        store.setHidden(e.calendarId, true)
        store.show(CalendarToast(
            message: "“\(e.calendar)” is hidden",
            detail: "Show it again from Calendars.",
            action: .init(label: "Undo") { store.setHidden(e.calendarId, false) }
        ))
    }

    /// Deletes an event (this time, or `all` of a series). Guests aren't emailed: the
    /// toast offers to tell them.
    func delete(_ e: CalendarEvent, all: Bool) {
        guard let api = model.api else { return }
        Task {
            do {
                let done = try await api.removeEvent(e.id, all: all)
                store.eventsChanged()
                offer(done.invitations, headline: "“\(e.title)” was deleted", note: done.note)
            } catch {
                fail(error)
            }
        }
    }

    /// After something the user did, once the sheet is gone: what happened, and a Send
    /// button when guests could be told.
    func offer(_ offers: [InvitationOffer], headline: String, note: String?) {
        guard let o = offers.first(where: { $0.sentAt == nil }) else {
            store.show(CalendarToast(message: headline, detail: note))
            return
        }
        let nothing = "Nothing was emailed to \(ScheduleWords.guestNames(o))."
        guard o.from != nil else {
            store.show(CalendarToast(message: headline, detail: "\(nothing) To send invitations from here, connect your email on your computer (Settings › Connections)."), seconds: 8)
            return
        }
        store.show(CalendarToast(
            message: headline,
            detail: "\(nothing) \(ScheduleWords.question(o))",
            action: .init(label: "Send") { send(o) }
        ), seconds: 20)
    }

    func send(_ o: InvitationOffer) {
        guard let api = model.api else { return }
        Task {
            do {
                let sent = try await api.sendInvitations(o.id)
                store.say(ScheduleWords.sentLabel(sent))
            } catch {
                fail(error)
            }
        }
    }

    func fail(_ error: Error) {
        store.show(CalendarToast(message: error.localizedDescription, isError: true), seconds: 6)
    }
}

/// The long-press menu of an event.
struct EventMenu: View {
    let event: CalendarEvent
    @Environment(AppModel.self) private var model
    @Environment(CalendarRouter.self) private var router
    private let store = CalendarStore.shared

    var body: some View {
        let e = event
        let actions = EventActions(model: model, store: store)
        let reminders = store.reminders(for: e.id)
        Button("Open", systemImage: "arrow.up.forward.app") { router.sheet = .event(e) }
        // Only before it starts: a reminder for something under way is refused.
        if e.start > actions.math.ms(Date()) {
            Menu {
                Section("Follows the event if it moves") {
                    ForEach(ScheduleWords.before, id: \.minutes) { b in
                        Button(b.label) { actions.remind(e, minutes: b.minutes) }
                    }
                }
            } label: {
                Label("Remind me", systemImage: "bell")
            }
        }
        if !reminders.isEmpty {
            Button(reminders.count > 1 ? "Remove its reminders" : "Remove its reminder", systemImage: "bell.slash") {
                actions.removeReminders(of: e)
            }
        }
        Button("Ask \(model.assistantName) about this", systemImage: "sparkles") { actions.ask(e) }
        Button("Copy details", systemImage: "doc.on.doc") { actions.copy(e) }
        if store.writable(e) {
            Section {
                Button("Edit", systemImage: "pencil") { router.sheet = .editEvent(e) }
                if e.canInvite {
                    Button("Send invitations…", systemImage: "paperplane") { router.sheet = .invite(e) }
                }
                Button("Delete…", systemImage: "trash", role: .destructive) { router.deleting = e }
            }
        }
        Section {
            Button("Hide “\(e.calendar)”", systemImage: "eye.slash") { actions.hide(e) }
        }
    }
}

/// Confirms deleting an event; a repeating one asks which: this time or every time.
struct DeleteEventConfirmation: ViewModifier {
    @Binding var event: CalendarEvent?
    let onDelete: (CalendarEvent, _ all: Bool) -> Void

    func body(content: Content) -> some View {
        content.confirmationDialog(
            "Delete “\(event?.title ?? "")”?",
            isPresented: Binding(get: { event != nil }, set: { if !$0 { event = nil } }),
            titleVisibility: .visible,
            presenting: event
        ) { e in
            if e.repeats {
                Button("Only this time", role: .destructive) { onDelete(e, false) }
                Button("Every time", role: .destructive) { onDelete(e, true) }
            } else {
                Button("Delete event", role: .destructive) { onDelete(e, false) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { e in
            let guests = e.canInvite ? " Your guests aren't emailed: you can tell them next." : ""
            Text(verbatim: (e.repeats ? "It repeats. Delete only this time, or every time?" : "It's removed from \(e.calendar).") + guests)
        }
    }
}
