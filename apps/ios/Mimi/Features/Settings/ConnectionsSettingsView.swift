import SwiftUI

/// The tinted icon that stands for an integration (the desktop's `IntegrationIcon`).
struct IntegrationIcon: View {
    let id: String
    var size: CGFloat = 32

    var body: some View {
        let look = Self.look(id)
        SettingsIcon(systemImage: look.symbol, tint: Color(hex: look.bg), foreground: Color(hex: look.fg), size: size)
    }

    static func look(_ id: String) -> (symbol: String, bg: UInt32, fg: UInt32) {
        switch id {
        case "google_calendar", "google", "caldav": ("calendar", 0xFDEBEA, 0xC9372A)
        case "google_contacts", "carddav": ("person.crop.circle.fill", 0xFBEEDD, 0x9A5A12)
        case "telegram": ("paperplane.fill", 0xDEF3F7, 0x136C86)
        case "signal": ("message.fill", 0xE5ECFB, 0x3353A8)
        case "matrix": ("number", 0xEDEDF0, 0x1D1D1F)
        case "whatsapp": ("phone.fill", 0xE1F4E6, 0x1D7A3A)
        case "email": ("envelope.fill", 0xEFE9FB, 0x6146AD)
        default: ("puzzlepiece.extension.fill", 0xEDEDF0, 0x1D1D1F)
        }
    }

    /// The integrations that can be connected from here.
    static func connectable(_ id: String) -> Bool {
        ["google_calendar", "caldav", "carddav", "telegram", "email"].contains(id)
    }

    /// Integrations that make sense to connect more than once.
    static func repeatable(_ id: String) -> Bool {
        ["google_calendar", "caldav", "carddav", "email"].contains(id)
    }
}

/// Settings › Connections: the user's own accounts, what's available, and what's coming.
struct ConnectionsSettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var integrations: [Integration] = []
    @State private var connections: [ConnectionItem] = []
    @State private var people: [PersonSummary] = []
    @State private var loaded = false
    @State private var open: Integration?
    @State private var removing: ConnectionItem?
    @State private var error: String?

    private var available: [Integration] {
        integrations.filter { $0.status == "available" || ($0.status == "connected" && IntegrationIcon.repeatable($0.id)) }
    }
    private var soon: [Integration] { integrations.filter { $0.status == "coming_soon" } }

    var body: some View {
        List {
            Section {
                Text("Let \(model.assistantName) help with the apps you use. It only sees what you connect, and always asks before it sends or changes anything.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
            }

            Section {
                Button { model.tab = .people } label: { peopleEntry }
                    .tint(.primary)
            }

            Section("Connected") {
                if connections.isEmpty {
                    ExplainedRow(systemImage: "puzzlepiece.extension", tint: Color(.tertiarySystemFill), foreground: .secondary,
                                 title: "Nothing connected yet",
                                 detail: loaded ? "Connect a calendar, your email or Telegram below to get started." : "Loading…")
                }
                ForEach(connections) { c in
                    connectionRow(c)
                }
            }

            if !available.isEmpty {
                Section("Available") {
                    ForEach(available) { i in
                        Button { open = i } label: {
                            HStack(spacing: 12) {
                                IntegrationIcon(id: i.id)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(i.name).foregroundStyle(Color.primary)
                                    Text(i.description).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                                }
                                Spacer(minLength: 8)
                                Text(i.status == "connected" ? "Add another" : "Connect")
                                    .font(.subheadline.weight(.semibold))
                                    .foregroundStyle(Color.ink)
                                    .padding(.horizontal, 12)
                                    .frame(height: 28)
                                    .background(Color(.tertiarySystemFill), in: .capsule)
                            }
                        }
                    }
                }
            }

            if !soon.isEmpty {
                Section("Coming soon") {
                    ForEach(soon) { i in
                        Button { open = i } label: {
                            HStack(spacing: 12) {
                                IntegrationIcon(id: i.id).opacity(0.55).saturation(0.6)
                                Text(i.name).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }

            Section {
            } footer: {
                Label("Sign-ins are stored encrypted on your computer. Disconnecting deletes them.", systemImage: "lock.fill")
                    .font(.footnote)
                    .frame(maxWidth: .infinity)
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Connections")
        .task(id: model.revision("connections_changed")) { await load() }
        .task(id: model.revision("people_changed")) {
            if let list = try? await model.api?.people("") { people = list }
        }
        .refreshable { await load() }
        .sheet(item: $open) { i in
            IntegrationSheet(integration: i)
        }
        .alert("Disconnect \(removing?.name ?? "")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } })) {
            Button("Cancel", role: .cancel) { removing = nil }
            Button("Disconnect", role: .destructive) {
                guard let c = removing else { return }
                removing = nil
                Task { await disconnect(c) }
            }
        } message: {
            Text("\(model.assistantName) loses access right away and the saved sign-in is deleted. Nothing is deleted on the service itself.")
        }
        .problemAlert($error)
    }

    private var peopleEntry: some View {
        HStack(spacing: 12) {
            if people.isEmpty {
                SettingsIcon(systemImage: "person.2.fill", tint: Color(.tertiarySystemFill), foreground: .secondary, size: 32)
            } else {
                AvatarStack(people: people.prefix(3).map { ($0.id, $0.name) }, size: 30)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text("People")
                Text(people.isEmpty
                     ? "Add the people you talk about, or connect an address book"
                     : "\(people.count) \(people.count == 1 ? "person" : "people") you can mention with @")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
        }
    }

    private func connectionRow(_ c: ConnectionItem) -> some View {
        HStack(spacing: 12) {
            IntegrationIcon(id: c.integration)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(c.name).lineLimit(1)
                    Circle().fill(statusColor(c.status)).frame(width: 6, height: 6)
                        .accessibilityLabel(c.status == .ok ? "Working" : c.status == .needsAction ? "Needs a step" : "Not working")
                }
                Text(c.detail)
                    .font(.footnote)
                    .foregroundStyle(c.status == .error ? Color.danger : c.status == .needsAction ? Color.cloudTone : .secondary)
                if c.integration == "google" && c.status == .error {
                    Text("Sign in again on your computer, in Settings › Connections.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 8)
            if let action = c.actionUrl, let url = URL(string: action) {
                Button("Finish setup") { openURL(url) }
                    .buttonStyle(.borderedProminent)
                    .tint(Color.lime)
                    .foregroundStyle(Color.limeInk)
                    .controlSize(.small)
            }
        }
        .swipeActions {
            Button("Disconnect", systemImage: "xmark.circle") { removing = c }.tint(.red)
        }
        .contextMenu {
            Button("Disconnect", systemImage: "xmark.circle", role: .destructive) { removing = c }
        }
    }

    private func statusColor(_ s: ConnectionStatus) -> Color {
        switch s {
        case .ok: .privateTone
        case .needsAction: .cloudTone
        case .error: .danger
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            async let i = api.integrations()
            async let c = api.connections()
            integrations = try await i
            connections = try await c
            loaded = true
        } catch {
            if !loaded { self.error = error.localizedDescription }
        }
    }

    private func disconnect(_ c: ConnectionItem) async {
        do {
            try await model.api?.disconnect(c.id)
            withAnimation { connections.removeAll { $0.id == c.id } }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// What an integration lets the assistant do, and the way in.
private struct IntegrationSheet: View {
    let integration: Integration
    @Environment(\.dismiss) private var dismiss
    @State private var connecting = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 22) {
                    IntegrationIcon(id: integration.id, size: 64)
                    VStack(spacing: 6) {
                        Text(integration.name).font(.title2.weight(.semibold))
                        Text(integration.description)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        Text("What your assistant can do")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(integration.abilities.enumerated()), id: \.offset) { i, a in
                                if i > 0 { Divider().padding(.leading, 40) }
                                HStack(alignment: .firstTextBaseline, spacing: 12) {
                                    Image(systemName: "checkmark")
                                        .font(.subheadline.weight(.bold))
                                        .foregroundStyle(Color.privateTone)
                                    Text(a).font(.subheadline)
                                    Spacer(minLength: 0)
                                }
                                .padding(.horizontal, 14)
                                .padding(.vertical, 12)
                            }
                        }
                        .background(Color(.secondarySystemGroupedBackground), in: .rect(cornerRadius: 16))
                    }
                    if integration.status == "coming_soon" {
                        Text("Coming in an upcoming update")
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 16)
                            .frame(height: 34)
                            .background(Color(.tertiarySystemFill), in: .capsule)
                    } else if IntegrationIcon.connectable(integration.id) {
                        Button(integration.status == "connected" ? "Add another" : "Connect \(integration.name)") {
                            connecting = true
                        }
                        .buttonStyle(SettingsInkButtonStyle())
                    }
                }
                .padding(24)
            }
            .background(Color(.systemGroupedBackground))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            }
            .navigationDestination(isPresented: $connecting) {
                ConnectFlowView(integration: integration.id) { dismiss() }
            }
        }
    }
}

/// The dark primary button (the desktop's `default`).
struct SettingsInkButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.semibold))
            .foregroundStyle(.white)
            .padding(.horizontal, 20)
            .frame(minHeight: 50)
            .frame(maxWidth: .infinity)
            .background(Color.ink, in: .capsule)
            .opacity(isEnabled ? 1 : 0.4)
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.7), value: configuration.isPressed)
    }
}
