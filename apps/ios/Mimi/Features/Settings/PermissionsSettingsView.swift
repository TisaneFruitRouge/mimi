import SwiftUI

/// Settings › Permissions: what the assistant may do without asking first, kind by kind.
/// The kinds come from the computer (`GET /v1/permissions`); this page only renders them.
struct PermissionsSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var kinds: [PermissionKind] = []
    @State private var loaded = false
    @State private var addingTo: PermissionKind?
    @State private var error: String?

    var body: some View {
        List {
            // A header, not a row: a row's narrow insets clip wrapped text at the edge.
            Section {} header: {
                Text("What \(model.assistantName) may do for you without asking first.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .textCase(nil)
            }
            ForEach(kinds) { k in
                Section {
                    kindRows(k)
                } footer: {
                    if let note = k.note { Text(note) }
                }
            }
            if loaded {
                Section {
                } footer: {
                    Text("Everything \(model.assistantName) does shows in the chat, whichever you choose. Emails, websites and messages from other people can hide instructions meant for \(model.assistantName); asking first is what stops them, so choose Automatic only for what you're comfortable with.")
                }
            } else {
                Section { ProgressView().frame(maxWidth: .infinity) }
            }
        }
        .listStyle(.insetGrouped)
        .listSectionSpacing(18)
        .navigationTitle("Permissions")
        .task(id: model.setting("permissions")) { await load() }
        .refreshable { await load() }
        .sheet(item: $addingTo) { k in
            ExceptionPicker(kind: k) { target, label in
                addingTo = nil
                let flipped: Autonomy = k.autonomy == .ask ? .automatic : .ask
                Task { await save(k, autonomy: k.autonomy, rules: k.rules + [PermissionRuleView(target: target, autonomy: flipped, label: label)]) }
            }
        }
        .problemAlert($error)
    }

    @ViewBuilder
    private func kindRows(_ k: PermissionKind) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                SettingsIcon(systemImage: Self.symbol(k.icon), tint: Color(hexString: k.color))
                Text(k.title).font(.headline)
            }
            Picker(k.title, selection: Binding(get: { k.autonomy }, set: { a in
                guard a != k.autonomy else { return }
                Task { await save(k, autonomy: a, rules: k.rules) }
            })) {
                ForEach(Autonomy.allCases, id: \.self) { Text($0.label).tag($0) }
            }
            .pickerStyle(.segmented)
            Text(k.detail)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .contentTransition(.opacity)
        }
        .padding(.vertical, 6)

        if k.targets != "none" {
            ForEach(k.rules, id: \.target) { r in
                exceptionRow(k, r)
            }
            Button {
                addingTo = k
            } label: {
                Label(k.targets == "person" ? "Add a person…" : "Add a calendar…", systemImage: "plus")
            }
        }
    }

    private func exceptionRow(_ k: PermissionKind, _ r: PermissionRuleView) -> some View {
        HStack(spacing: 12) {
            if r.target.isPerson, let id = UUID(uuidString: r.target.id) {
                PersonAvatar(id: id, name: r.label, size: 28)
            } else {
                Image(systemName: "calendar")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: 28, height: 28)
                    .background(Color(.tertiarySystemFill), in: .rect(cornerRadius: 7))
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(r.label).foregroundStyle(r.missing ? .secondary : .primary)
                if r.missing {
                    Text("This exception no longer applies.").font(.footnote).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 8)
            if !r.missing {
                Menu {
                    Picker("For \(r.label)", selection: Binding(get: { r.autonomy }, set: { a in
                        guard a != r.autonomy else { return }
                        let rules = k.rules.map { $0.target == r.target ? PermissionRuleView(target: $0.target, autonomy: a, label: $0.label, missing: $0.missing) : $0 }
                        Task { await save(k, autonomy: k.autonomy, rules: rules) }
                    })) {
                        ForEach(Autonomy.allCases, id: \.self) { Text($0.label).tag($0) }
                    }
                    Divider()
                    Button("Remove exception", systemImage: "minus.circle", role: .destructive) {
                        Task { await save(k, autonomy: k.autonomy, rules: k.rules.filter { $0.target != r.target }) }
                    }
                } label: {
                    HStack(spacing: 3) {
                        Text(r.autonomy.label)
                        Image(systemName: "chevron.up.chevron.down").font(.caption2)
                    }
                    .font(.subheadline)
                }
                .accessibilityLabel("\(k.title) for \(r.label): \(r.autonomy.label)")
            }
        }
        .swipeActions {
            Button("Remove", systemImage: "trash") {
                Task { await save(k, autonomy: k.autonomy, rules: k.rules.filter { $0.target != r.target }) }
            }
            .tint(.red)
        }
    }

    /// The daemon names lucide icons; these are the SF Symbols closest to them.
    static func symbol(_ lucide: String) -> String {
        switch lucide {
        case "send": "paperplane.fill"
        case "calendar-plus": "calendar.badge.plus"
        case "calendar-cog": "calendar.badge.clock"
        case "bell-ring": "bell.and.waves.left.and.right.fill"
        case "mail": "envelope.fill"
        default: "checkmark.shield.fill"
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            kinds = try await api.permissions()
            loaded = true
        } catch {
            if !loaded { self.error = error.localizedDescription }
        }
    }

    private func save(_ k: PermissionKind, autonomy: Autonomy, rules: [PermissionRuleView]) async {
        guard let api = model.api else { return }
        do {
            let next = try await api.setPermission(k.id, KindPermission(
                autonomy: autonomy,
                rules: rules.map { KindPermission.Rule(target: $0.target, autonomy: $0.autonomy) }
            ))
            withAnimation { kinds = next }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// "Add a person…" / "Add a calendar…": a searchable list of who or what can be an
/// exception (people need an email address).
private struct ExceptionPicker: View {
    let kind: PermissionKind
    let onPick: (PermissionTarget, String) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var options: [(target: PermissionTarget, label: String, person: UUID?, color: String?)] = []
    @State private var loaded = false
    @State private var query = ""

    private var person: Bool { kind.targets == "person" }

    private var shown: [(target: PermissionTarget, label: String, person: UUID?, color: String?)] {
        let taken = Set(kind.rules.map(\.target))
        let q = query.trimmingCharacters(in: .whitespaces)
        return options.filter { !taken.contains($0.target) && (q.isEmpty || $0.label.localizedCaseInsensitiveContains(q)) }
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(shown, id: \.target) { o in
                        Button { onPick(o.target, o.label) } label: {
                            HStack(spacing: 12) {
                                if let id = o.person {
                                    PersonAvatar(id: id, name: o.label, size: 30)
                                } else {
                                    Image(systemName: "calendar")
                                        .foregroundStyle(Color(hexString: o.color, fallback: .secondary))
                                        .frame(width: 30)
                                }
                                Text(o.label).foregroundStyle(Color.primary)
                            }
                        }
                    }
                } header: {
                    Text(kind.autonomy == .ask ? "Without asking, for:" : "Always ask first, for:")
                }
            }
            .overlay {
                if loaded && shown.isEmpty {
                    ContentUnavailableView(person ? "No one with an email address" : "No calendars",
                                           systemImage: person ? "person.crop.circle.badge.questionmark" : "calendar")
                }
            }
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always),
                        prompt: person ? "Search people" : "Search calendars")
            .navigationTitle(kind.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            .task { await load() }
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        if person {
            let people = (try? await api.people("")) ?? []
            options = people.filter { $0.channels.contains(.email) }.map {
                (PermissionTarget(kind: "person", id: $0.id.mimiPath), $0.name, $0.id, nil)
            }
        } else {
            let calendars = (try? await api.calendarsForPermissions()) ?? []
            options = calendars.map { (PermissionTarget(kind: "calendar", id: $0.id), $0.name, nil, $0.color) }
        }
        loaded = true
    }
}
