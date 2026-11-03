import SwiftUI

/// What the Calendar tab has open: one sheet at a time, a confirmation, a conversation.
@Observable
final class CalendarRouter {
    enum Sheet: Identifiable {
        case event(CalendarEvent)
        case newEvent(start: Int64?)
        case editEvent(CalendarEvent)
        case invite(CalendarEvent)
        case newItem(ScheduleDraft?)
        case editItem(ScheduleItem)
        case calendars

        var id: String {
            switch self {
            case .event(let e): "event-\(e.id)"
            case .newEvent(let s): "new-\(s ?? 0)"
            case .editEvent(let e): "edit-\(e.id)"
            case .invite(let e): "invite-\(e.id)"
            case .newItem: "new-item"
            case .editItem(let i): "item-\(i.id)"
            case .calendars: "calendars"
            }
        }
    }

    var sheet: Sheet?
    /// An event whose deletion waits for the user's confirmation.
    var deleting: CalendarEvent?
    /// A conversation to show (a routine's results).
    var conversation: UUID?
    var showReminders = false
}

/// The bottom message: what just happened, with Undo or Send when there's something to do.
struct CalendarToastView: View {
    @Bindable var store: CalendarStore

    var body: some View {
        ZStack {
            if let toast = store.toast {
                HStack(spacing: 12) {
                    Image(systemName: toast.isError ? "exclamationmark.circle.fill" : "checkmark.circle.fill")
                        .foregroundStyle(toast.isError ? Color.danger : Color.privateTone)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: toast.message)
                            .font(.subheadline.weight(.semibold))
                            .lineLimit(2)
                        if let detail = toast.detail {
                            Text(verbatim: detail)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                                .lineLimit(3)
                        }
                    }
                    Spacer(minLength: 4)
                    if let action = toast.action {
                        Button(action.label) {
                            action.run()
                            if store.toast?.id == toast.id { store.toast = nil }
                        }
                        .font(.subheadline.weight(.semibold))
                        .buttonStyle(.bordered)
                        .buttonBorderShape(.capsule)
                        .tint(Color.ink)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .glassEffect(.regular, in: .rect(cornerRadius: 22))
                .padding(.horizontal, 14)
                .padding(.bottom, 8)
                .transition(.move(edge: .bottom).combined(with: .opacity))
                .id(toast.id)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("calendar-toast")
            }
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.82), value: store.toast)
    }
}

/// A reminder going off (or a routine finishing) while the app is open, with Done and
/// Snooze. The same thing also reaches Telegram and the computer.
struct DeliveryBanner: View {
    @Environment(AppModel.self) private var model
    var onOpenConversation: (UUID) -> Void
    private let math = CalendarMath()

    private var delivery: Delivery? {
        guard let json = model.delivery, let data = try? JSONEncoder().encode(json) else { return nil }
        return try? JSONDecoder().decode(Delivery.self, from: data)
    }

    var body: some View {
        ZStack {
            if let d = delivery, let text = headline(d) {
                HStack(alignment: .center, spacing: 12) {
                    CalendarIconTile(systemName: d.kind == .routine ? "sparkles" : "bell.fill", tint: d.kind == .routine ? .limeDeep : .ink, fill: d.kind == .routine ? .limeSoft : Color.subtle)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(verbatim: text).font(.subheadline.weight(.semibold)).lineLimit(2)
                        Text(subline(d)).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                    }
                    Spacer(minLength: 4)
                    actions(d)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .glassEffect(.regular, in: .rect(cornerRadius: 22))
                .padding(.horizontal, 12)
                // Swipe it up to put it away.
                .gesture(DragGesture(minimumDistance: 12).onEnded { value in
                    if value.translation.height < -16 { model.delivery = nil }
                })
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: model.delivery)
    }

    private func headline(_ d: Delivery) -> String? {
        switch (d.kind, d.status) {
        case (.reminder, .delivered), (.reminder, .late): d.title
        case (.routine, .delivered), (.routine, .late): "\(d.title) is ready"
        case (.routine, .failed): "\(d.title) didn't finish"
        default: nil
        }
    }

    private func subline(_ d: Delivery) -> String {
        if d.status == .late { return "Late: it was due at \(math.clock(d.dueAt))" }
        if d.status == .failed { return d.detail ?? "Something went wrong." }
        return d.kind == .routine ? "Routine" : "Reminder"
    }

    @ViewBuilder
    private func actions(_ d: Delivery) -> some View {
        if d.kind == .reminder {
            Menu {
                Button("In 10 minutes") { act { try await $0.snoozeReminder(d.id, minutes: 10) } }
                Button("In 1 hour") { act { try await $0.snoozeReminder(d.id, minutes: 60) } }
            } label: {
                Image(systemName: "clock")
                    .font(.subheadline.weight(.semibold))
                    .frame(width: 34, height: 34)
                    .background(Color.subtle, in: .circle)
            }
            .accessibilityLabel("Snooze")
            Button("Done") { act { try await $0.reminderDone(d.id) } }
                .font(.subheadline.weight(.semibold))
                .buttonStyle(.borderedProminent)
                .buttonBorderShape(.capsule)
                .tint(Color.ink)
        } else if let c = d.conversationId, d.status != .failed {
            Button("Open") {
                model.delivery = nil
                onOpenConversation(c)
            }
            .font(.subheadline.weight(.semibold))
            .buttonStyle(.bordered)
            .buttonBorderShape(.capsule)
            .tint(Color.ink)
        } else {
            Button("Close", systemImage: "xmark") { model.delivery = nil }
                .labelStyle(.iconOnly)
                .foregroundStyle(.secondary)
        }
    }

    private func act(_ run: @escaping (MimiAPI) async throws -> Void) {
        guard let api = model.api else { return }
        model.delivery = nil
        Task {
            do { try await run(api) } catch {
                CalendarStore.shared.show(CalendarToast(message: error.localizedDescription, isError: true))
            }
        }
    }
}

/// A tinted rounded-square icon, like the desktop's `IconTile`.
struct CalendarIconTile: View {
    let systemName: String
    var tint: Color = .ink
    var fill: Color = .subtle
    var size: CGFloat = 32

    var body: some View {
        Image(systemName: systemName)
            .font(.system(size: size * 0.45, weight: .semibold))
            .foregroundStyle(tint)
            .frame(width: size, height: size)
            .background(fill, in: .rect(cornerRadius: size * 0.3))
    }
}

/// A small capsule label ("Paused").
struct CalendarPill: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(Color.subtle, in: .capsule)
    }
}
