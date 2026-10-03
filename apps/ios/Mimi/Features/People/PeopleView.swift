import SwiftUI

/// Everyone the assistant can mention and reach: searchable, in letter sections like
/// Contacts, with possible duplicates on top. Select several to merge them.
struct PeopleView: View {
    @Environment(AppModel.self) private var model
    @State private var flow = PeopleFlow()
    @State private var query = ""
    @State private var people: [PersonSummary] = []
    @State private var loaded = false
    @State private var duplicates: [DuplicateSuggestion] = []
    @State private var adding = false
    @State private var editMode: EditMode = .inactive
    @State private var selection = Set<UUID>()
    /// The order people were chosen in: the first one is kept by a merge.
    @State private var chosenOrder: [UUID] = []

    private var groups: [PeopleIndex.Group] { PeopleIndex.groups(people) }
    private var selecting: Bool { editMode.isEditing }

    var body: some View {
        NavigationStack(path: $flow.path) {
            List(selection: $selection) {
                if !selecting && query.isEmpty && !duplicates.isEmpty {
                    Section {
                        NavigationLink(value: PeopleRoute.duplicates) {
                            Label {
                                Text(duplicates.count == 1 ? "1 possible duplicate" : "\(duplicates.count) possible duplicates")
                                    .foregroundStyle(Color.cloudTone)
                            } icon: {
                                Image(systemName: "person.2.fill").foregroundStyle(Color.cloudTone)
                            }
                        }
                        .listRowBackground(Color.cloudSoft)
                    }
                }
                ForEach(groups) { group in
                    Section {
                        ForEach(group.people) { p in
                            row(p)
                        }
                    } header: {
                        Text(group.letter)
                    }
                    .sectionIndexLabel(group.letter)
                }
            }
            .listStyle(.insetGrouped)
            .listSectionIndexVisibility(query.isEmpty && people.count > 12 ? .visible : .hidden)
            .environment(\.editMode, $editMode)
            .searchable(text: $query, prompt: "Name, number or email")
            .navigationTitle(selecting ? selectionTitle : "People")
            .navigationBarTitleDisplayMode(selecting ? .inline : .large)
            .toolbar { toolbar }
            .refreshable { await sync() }
            .overlay { emptyState }
            // Above the tab bar: a bottom toolbar would sit behind it and never show.
            .safeAreaInset(edge: .bottom) {
                if selecting { mergeBar }
            }
            .navigationDestination(for: PeopleRoute.self) { route in
                switch route {
                case .person(let id): PersonView(id: id)
                case .duplicates: DuplicatesView(pairs: duplicates)
                case .removed: RemovedContactsView()
                case .conversation(let id): ChatView(conversationId: id)
                case .memoryNote(let path): MemoryNoteView(path: path)
                }
            }
            .task(id: LoadKey(query: query, revision: model.revision("people_changed"))) { await load() }
            .onChange(of: selection) { _, now in
                chosenOrder = chosenOrder.filter(now.contains) + now.filter { !chosenOrder.contains($0) }
            }
            .onChange(of: editMode.isEditing) { _, editing in
                if !editing { selection = []; chosenOrder = [] }
            }
            .sheet(isPresented: $adding) {
                AddPersonSheet { person in flow.open(person.id) }
            }
        }
        .environment(flow)
        .modifier(PeopleFlowSheets(flow: flow))
        // Someone opened from another panel (a guest, a sender).
        .onAppear { takeOpenPerson() }
        .onChange(of: model.openPerson) { _, _ in takeOpenPerson() }
    }

    private func takeOpenPerson() {
        guard let id = model.openPerson else { return }
        model.openPerson = nil
        flow.open(id)
    }

    private var selectionTitle: String {
        selection.isEmpty ? "Choose people" : selection.count == 1 ? "1 selected" : "\(selection.count) selected"
    }

    @ViewBuilder
    private func row(_ p: PersonSummary) -> some View {
        NavigationLink(value: PeopleRoute.person(p.id)) {
            HStack(spacing: 12) {
                PersonAvatar(id: p.id, name: p.name, size: 38)
                VStack(alignment: .leading, spacing: 1) {
                    Text(p.name).lineLimit(1)
                    if let nick = p.nickname, !nick.isEmpty {
                        Text(nick).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                ChannelGlyphs(channels: p.channels)
            }
            .padding(.vertical, 1)
        }
        .swipeActions(edge: .trailing) {
            Button("Delete", systemImage: "trash") { flow.deleting = p }
                .tint(.red)
        }
        .swipeActions(edge: .leading) {
            Button("Merge", systemImage: "arrow.triangle.merge") { flow.pickingFor = p }
                .tint(Color(hex: 0x5856D6))
        }
        .contextMenu {
            Button("Ask \(model.assistantName) about \(p.nickname ?? p.name)", systemImage: "sparkles") {
                model.ask(about: .person, id: p.id.mimiPath, label: p.name)
            }
            Button("Merge with…", systemImage: "arrow.triangle.merge") { flow.pickingFor = p }
            Divider()
            Button("Delete contact…", systemImage: "trash", role: .destructive) { flow.deleting = p }
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        if selecting {
            ToolbarItem(placement: .topBarLeading) {
                Button("Cancel") { withAnimation { editMode = .inactive } }
            }
        } else {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Select to merge", systemImage: "checkmark.circle") { withAnimation { editMode = .active } }
                        .disabled(people.count < 2)
                    Button("Refresh from address books", systemImage: "arrow.clockwise") { Task { await sync() } }
                    NavigationLink(value: PeopleRoute.removed) {
                        Label("Removed contacts", systemImage: "person.crop.circle.badge.xmark")
                    }
                } label: {
                    Image(systemName: "ellipsis")
                }
                .accessibilityLabel("More")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button("Add someone", systemImage: "plus") { adding = true }
                    .disabled(model.phase != .online)
            }
        }
    }

    private var mergeBar: some View {
        VStack(spacing: 6) {
            Button {
                guard let keep = chosenOrder.first else { return }
                flow.plan = MergePlan(keep: keep, others: Array(chosenOrder.dropFirst()))
                withAnimation { editMode = .inactive }
            } label: {
                Label(selection.count > 1 ? "Merge \(selection.count) contacts" : "Merge contacts", systemImage: "arrow.triangle.merge")
            }
            .buttonStyle(.lime)
            .disabled(selection.count < 2)
            if selection.count < 2 {
                Text("Choose two or more people who are the same person.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 8)
        .padding(.bottom, 4)
        .transition(.move(edge: .bottom).combined(with: .opacity))
    }

    @ViewBuilder
    private var emptyState: some View {
        if loaded && people.isEmpty {
            if query.isEmpty {
                ContentUnavailableView {
                    Label("No one here yet", systemImage: "person.2")
                } description: {
                    Text("Connect an iCloud, Fastmail or Nextcloud account on the Connections page and its address book appears here. You can also add people yourself.")
                } actions: {
                    Button("Add someone") { adding = true }
                        .buttonStyle(.borderedProminent)
                }
            } else {
                ContentUnavailableView.search(text: query)
            }
        }
    }

    private struct LoadKey: Equatable {
        var query: String
        var revision: Int
    }

    private func load() async {
        guard let api = model.api else { return }
        // Typing: wait for a pause before asking the computer.
        if !query.isEmpty { try? await Task.sleep(for: .milliseconds(250)) }
        guard !Task.isCancelled else { return }
        do {
            async let list = api.people(query.trimmingCharacters(in: .whitespaces))
            async let dupes = api.duplicates()
            people = try await list
            duplicates = (try? await dupes) ?? []
            loaded = true
        } catch is CancellationError {
        } catch {
            if !Task.isCancelled { flow.error = error.localizedDescription }
        }
    }

    private func sync() async {
        do {
            try await model.api?.syncPeople()
            await load()
        } catch {
            flow.error = error.localizedDescription
        }
    }
}
