import SwiftUI

/// Settings › Reminders & notifications: where reminders and routine results reach the
/// user, and what went off lately (with Done and Snooze for the latest).
struct NotificationsSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var store = CalendarStore.shared
    @State private var computerNotifications: Bool?
    @State private var busy = false
    @State private var conversation: UUID?
    @State private var error: String?
    private let math = CalendarMath()

    var body: some View {
        List {
            Section {
                computerRow
                HStack(spacing: 12) {
                    CalendarIconTile(systemName: "paperplane.fill", tint: .networkTone, fill: .networkSoft)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Telegram")
                        Text(store.telegramConnected
                            ? "Sent to your bot, with Done and Snooze buttons. Routines ask for your OK there too."
                            : "Connect a Telegram bot on your computer to get them on your phone, even when this app is closed.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 4)
                    if store.telegramConnected { CalendarPill(text: "On") }
                }
                HStack(spacing: 12) {
                    CalendarIconTile(systemName: "iphone", tint: .ink, fill: .subtle)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("This phone")
                        Text("While the app is open, they show at the top of Calendar. Notifications on this phone aren't available yet.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
            } header: {
                Text("Where they reach you")
            }

            Section("Your reminders and routines") {
                NavigationLink {
                    RemindersView()
                } label: {
                    HStack(spacing: 12) {
                        CalendarIconTile(systemName: "bell.fill", tint: .ink, fill: .subtle)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Reminders and routines")
                            Text(summary).font(.footnote).foregroundStyle(.secondary)
                        }
                    }
                }
                Button {
                    model.tab = .calendar
                } label: {
                    HStack(spacing: 12) {
                        CalendarIconTile(systemName: "calendar", tint: .networkTone, fill: .networkSoft)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("In Calendar").foregroundStyle(.primary)
                            Text("See them next to your events").font(.footnote).foregroundStyle(.secondary)
                        }
                    }
                }
            }

            if !store.deliveries.isEmpty {
                Section("Recently") {
                    ForEach(store.deliveries.prefix(20)) { d in
                        DeliveryRow(
                            delivery: d,
                            latest: store.deliveries.first { $0.itemId == d.itemId } == d,
                            open: { conversation = $0 },
                            reload: reload
                        )
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Reminders & notifications")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $conversation) { ChatView(conversationId: $0) }
        .overlay(alignment: .bottom) { CalendarToastView(store: store) }
        .refreshable { await load() }
        .task(id: model.revision("schedule_changed")) { await load() }
        .task(id: model.revision("connections_changed")) {
            if let api = model.api { await store.loadConnections(api) }
        }
        .task { await loadSettings() }
        .alert("Something went wrong", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    private var summary: String {
        let active = store.items.filter { !$0.finished }
        let reminders = active.filter { $0.kind == .reminder }.count
        let routines = active.filter { $0.kind == .routine }.count
        if reminders + routines == 0 { return "Nothing planned" }
        var parts: [String] = []
        if reminders > 0 { parts.append(reminders == 1 ? "1 reminder" : "\(reminders) reminders") }
        if routines > 0 { parts.append(routines == 1 ? "1 routine" : "\(routines) routines") }
        return parts.joined(separator: ", ")
    }

    /// Notifications on the computer: a setting of the computer, switched from here.
    private var computerRow: some View {
        let on = computerNotifications ?? true
        return Toggle(isOn: Binding(get: { on }, set: { new in Task { await setComputer(new) } })) {
            HStack(spacing: 12) {
                CalendarIconTile(systemName: "desktopcomputer", tint: .ink, fill: .subtle)
                VStack(alignment: .leading, spacing: 2) {
                    Text("On your computer")
                    Text(on ? "Shown even when the app is closed there" : "Off. They still appear in the app.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .disabled(busy || computerNotifications == nil)
        .accessibilityIdentifier("computer-notifications")
    }

    private func loadSettings() async {
        guard let api = model.api, let json = try? await api.settingsJSON() else { return }
        computerNotifications = json["desktop_notifications"]?.bool ?? true
    }

    private func setComputer(_ on: Bool) async {
        busy = true
        defer { busy = false }
        computerNotifications = on
        do {
            try await model.updateSettings(["desktop_notifications": .bool(on)])
        } catch {
            computerNotifications = !on
            self.error = error.localizedDescription
        }
        await loadSettings()
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            async let deliveries: Void = store.loadDeliveries(api)
            async let items: Void = store.loadItems(api)
            _ = try await (deliveries, items)
        } catch is CancellationError {
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func reload() {
        Task { await load() }
    }
}

/// One time something went off: when, what happened; Done and Snooze on the latest of a
/// reminder still waiting; "Open" for a routine's result.
private struct DeliveryRow: View {
    let delivery: Delivery
    /// The newest delivery of its reminder: the only one worth acting on.
    let latest: Bool
    let open: (UUID) -> Void
    let reload: () -> Void
    @Environment(AppModel.self) private var model
    private let math = CalendarMath()

    var body: some View {
        let d = delivery
        let conversation = d.status != .running ? d.conversationId : nil
        let pending = latest && d.kind == .reminder && (d.status == .delivered || d.status == .late)
        HStack(spacing: 12) {
            icon
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: d.title).lineLimit(2)
                Text(verbatim: detail).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
            }
            Spacer(minLength: 4)
            if pending {
                Menu {
                    Button("In 10 minutes") { act { try await $0.snoozeReminder(d.id, minutes: 10) } }
                    Button("In 1 hour") { act { try await $0.snoozeReminder(d.id, minutes: 60) } }
                } label: {
                    Image(systemName: "clock")
                        .font(.subheadline.weight(.semibold))
                        .frame(width: 32, height: 32)
                        .background(Color.subtle, in: .circle)
                }
                .accessibilityLabel("Snooze")
                Button("Done") { act { try await $0.reminderDone(d.id) } }
                    .font(.subheadline.weight(.semibold))
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .tint(Color.ink)
            } else if let conversation {
                Button("Open") { open(conversation) }
                    .font(.subheadline)
                    .buttonStyle(.borderless)
            }
        }
    }

    private var detail: String {
        let d = delivery
        var parts = [math.when(d.at, now: math.ms(Date()))]
        if d.status == .late { parts[0] += " (due \(math.clock(d.dueAt)))" }
        parts.append(ScheduleWords.status(d.status))
        let bad = d.status == .missed || d.status == .failed || d.status == .skipped
        if bad, let more = d.detail { parts.append(more) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    @ViewBuilder
    private var icon: some View {
        switch delivery.status {
        case .done: CalendarIconTile(systemName: "checkmark", tint: .privateTone, fill: .privateSoft, size: 28)
        case .missed, .failed, .skipped: CalendarIconTile(systemName: "exclamationmark", tint: .secondary, fill: .subtle, size: 28)
        case .snoozed: CalendarIconTile(systemName: "clock", tint: .secondary, fill: .subtle, size: 28)
        default: CalendarIconTile(systemName: delivery.kind == .routine ? "sparkles" : "bell.fill", tint: .secondary, fill: .subtle, size: 28)
        }
    }

    private func act(_ run: @escaping (MimiAPI) async throws -> Void) {
        guard let api = model.api else { return }
        Task {
            do {
                try await run(api)
                reload()
            } catch {
                CalendarStore.shared.show(CalendarToast(message: error.localizedDescription, isError: true))
            }
        }
    }
}
