import SwiftUI

/// "Merge with…": everyone else, searchable by name, number or address, to pick the
/// person who is the same as `person`.
struct MergePicker: View {
    let person: PersonSummary
    let onPick: (UUID) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var people: [PersonSummary] = []
    @State private var loaded = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(people.filter { $0.id != person.id }) { p in
                        Button { onPick(p.id) } label: {
                            HStack(spacing: 12) {
                                PersonAvatar(id: p.id, name: p.name, size: 36)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(p.name).foregroundStyle(Color.primary)
                                    Text(p.reachLine).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                                }
                            }
                        }
                    }
                } header: {
                    Text("Choose the contact that is the same person.")
                        .textCase(nil)
                }
            }
            .overlay {
                if loaded && people.filter({ $0.id != person.id }).isEmpty {
                    ContentUnavailableView.search(text: query)
                }
            }
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Name, number or email")
            .navigationTitle("Merge \(person.name) with…")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            // The computer searches names, numbers and addresses: no filtering here.
            .task(id: query) {
                if !query.isEmpty { try? await Task.sleep(for: .milliseconds(250)) }
                guard !Task.isCancelled, let api = model.api else { return }
                if let found = try? await api.people(query) {
                    people = found
                    loaded = true
                }
            }
        }
    }
}

/// The confirmation before merging: the name to keep and every way to reach the merged
/// person, each once. A notice offers Undo right after.
struct MergeSheet: View {
    let plan: MergePlan
    let onMerged: (MergeResult) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var preview: MergePreview?
    @State private var loadError: String?
    /// The chosen name; "" means "another name", typed below.
    @State private var choice: String?
    @State private var typed = ""
    @State private var busy = false
    @State private var error: String?

    private var picked: String { choice ?? preview?.names.first ?? "" }
    private var name: String { (picked.isEmpty ? typed : picked).trimmingCharacters(in: .whitespaces) }

    var body: some View {
        NavigationStack {
            Form {
                if let loadError {
                    Section { Text(loadError).foregroundStyle(Color.danger) }
                } else if let preview {
                    Section {
                        VStack(spacing: 10) {
                            AvatarStack(people: preview.people.map { ($0.id, $0.name) }, size: 48)
                            Text(preview.people.map(\.name).joined(separator: ", "))
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .multilineTextAlignment(.center)
                            Text("They become one person, with every way to reach them. Your address books aren't changed.")
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                                .multilineTextAlignment(.center)
                        }
                        .frame(maxWidth: .infinity)
                        .listRowBackground(Color.clear)
                    }

                    Section("Name") {
                        ForEach(preview.names, id: \.self) { n in
                            nameRow(n, checked: picked == n) { choice = n }
                        }
                        nameRow("Another name…", checked: picked.isEmpty) { choice = "" }
                        if picked.isEmpty {
                            TextField("Another name", text: $typed)
                        }
                    }

                    Section {
                        if preview.handles.isEmpty {
                            Text("No number or address yet.").foregroundStyle(.secondary)
                        }
                        ForEach(preview.handles) { h in
                            HStack(spacing: 12) {
                                ChannelGlyph(channel: h.channel, size: 15).frame(width: 24)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(h.value)
                                    Text(handleDetail(h)).font(.footnote).foregroundStyle(.secondary)
                                }
                            }
                        }
                    } header: {
                        Text("How to reach them")
                    } footer: {
                        Text("Changed your mind later? Open the merged contact and choose “Not the same person” on any of its cards.")
                    }
                    if let error {
                        Section { Text(error).foregroundStyle(Color.danger).font(.subheadline) }
                    }
                } else {
                    Section { ProgressView().frame(maxWidth: .infinity) }
                }
            }
            .navigationTitle("Merge \(plan.others.count + 1) contacts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Merge") { Task { await merge() } }
                        .disabled(busy || preview == nil || name.isEmpty)
                }
            }
            .task {
                do {
                    preview = try await model.api?.mergePreview(MergeRequest(keep: plan.keep, others: plan.others, name: nil))
                } catch {
                    loadError = error.localizedDescription
                }
            }
        }
    }

    private func nameRow(_ text: String, checked: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                Text(text).foregroundStyle(text == "Another name…" ? Color.secondary : Color.primary)
                Spacer()
                if checked {
                    Image(systemName: "checkmark").font(.body.weight(.semibold)).foregroundStyle(Color.limeDeep)
                }
            }
            .contentShape(.rect)
        }
        .accessibilityAddTraits(checked ? .isSelected : [])
    }

    private func merge() async {
        guard let api = model.api, !name.isEmpty else { return }
        busy = true
        defer { busy = false }
        do {
            let done = try await api.merge(MergeRequest(keep: plan.keep, others: plan.others, name: name))
            onMerged(done)
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// People who share a name: merge them, or say they're not the same.
struct DuplicatesView: View {
    @State var pairs: [DuplicateSuggestion]
    @Environment(AppModel.self) private var model
    @Environment(PeopleFlow.self) private var flow
    @State private var busy: String?

    var body: some View {
        List {
            Section {
                ForEach(pairs) { pair in
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(spacing: 12) {
                            AvatarStack(people: [(pair.a.id, pair.a.name), (pair.b.id, pair.b.name)], size: 34)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(pair.a.name == pair.b.name ? pair.a.name : "\(pair.a.name) and \(pair.b.name)")
                                    .font(.body.weight(.medium))
                                Text("\(pair.a.reachLine)\n\(pair.b.reachLine)")
                                    .font(.footnote).foregroundStyle(.secondary)
                            }
                        }
                        HStack(spacing: 10) {
                            Button("Not the same") { Task { await dismiss(pair) } }
                                .buttonStyle(.bordered)
                            Button("Same person") { Task { await merge(pair) } }
                                .buttonStyle(.borderedProminent)
                                .tint(Color.ink)
                        }
                        .controlSize(.small)
                        .disabled(busy == pair.id)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                    }
                    .padding(.vertical, 4)
                }
            } footer: {
                Text("These share a name. If they're the same person, combine them so \(model.assistantName) sees one person with every way to reach them.")
            }
        }
        .overlay {
            if pairs.isEmpty {
                ContentUnavailableView("No possible duplicates", systemImage: "checkmark.circle",
                                       description: Text("Everyone here looks like a different person."))
            }
        }
        .navigationTitle("Possible duplicates")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: model.revision("people_changed")) {
            if let fresh = try? await model.api?.duplicates() { withAnimation { pairs = fresh } }
        }
    }

    private func dismiss(_ pair: DuplicateSuggestion) async {
        busy = pair.id
        defer { busy = nil }
        do {
            try await model.api?.dismissDuplicate(pair.a.id, pair.b.id)
            withAnimation { pairs.removeAll { $0.id == pair.id } }
        } catch {
            flow.error = error.localizedDescription
        }
    }

    /// Same name: nothing to choose, so no confirmation. Undo is in the notice.
    private func merge(_ pair: DuplicateSuggestion) async {
        guard let api = model.api else { return }
        busy = pair.id
        defer { busy = nil }
        do {
            let done = try await api.merge(MergeRequest(keep: pair.a.id, others: [pair.b.id], name: nil))
            withAnimation { pairs.removeAll { $0.id == pair.id } }
            flow.merged(done, api: api)
        } catch {
            flow.error = error.localizedDescription
        }
    }
}

/// People deleted from Mimi, each with a way to bring them back.
struct RemovedContactsView: View {
    @Environment(AppModel.self) private var model
    @Environment(PeopleFlow.self) private var flow
    @State private var removed: [RemovedPerson] = []
    @State private var loaded = false
    @State private var busy: UUID?

    var body: some View {
        List {
            Section {
                ForEach(removed) { r in
                    HStack(spacing: 12) {
                        PersonAvatar(id: r.id, name: r.name, size: 36)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(r.name)
                            Text(detail(r)).font(.footnote).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 8)
                        Button("Bring back") { Task { await restore(r) } }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                            .disabled(busy != nil)
                    }
                }
            } footer: {
                if !removed.isEmpty {
                    Text("People you deleted from Mimi. Your address books and email still have them; bring someone back to see them here again.")
                }
            }
        }
        .overlay {
            if loaded && removed.isEmpty {
                ContentUnavailableView("No one has been removed", systemImage: "person.crop.circle.badge.checkmark")
            }
        }
        .navigationTitle("Removed contacts")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: model.revision("people_changed")) {
            if let list = try? await model.api?.removedPeople() {
                removed = list
                loaded = true
            }
        }
    }

    private func detail(_ r: RemovedPerson) -> String {
        let date = Date(timeIntervalSince1970: Double(r.removedAt) / 1000).formatted(.dateTime.day().month(.abbreviated))
        return r.sources.isEmpty ? "Removed \(date)" : "From \(r.sources.joined(separator: ", ")) · removed \(date)"
    }

    private func restore(_ r: RemovedPerson) async {
        busy = r.id
        defer { busy = nil }
        do {
            guard let api = model.api else { return }
            let p = try await api.restorePerson(r.id)
            flow.open(p.id)
            flow.notice = UndoNotice(text: "\(p.name) is back")
        } catch {
            flow.error = error.localizedDescription
        }
    }
}
