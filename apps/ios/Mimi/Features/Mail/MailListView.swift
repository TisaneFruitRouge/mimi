import SwiftUI

/// The conversations in a view or a smart folder, newest first, with search (through all
/// mail), swipe actions and a menu on each.
struct MailListView: View {
    let route: MailRoute
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store
    @State private var threads: [MailThread] = []
    @State private var loaded = false
    @State private var more = true
    @State private var loadingMore = false
    @State private var search = ""
    @State private var deleting: MailThread?
    @State private var editingFolder: MailFolder?
    @State private var deletingFolder: MailFolder?

    private static let page = 60

    private var box: MailBox? { if case .box(let b) = route { b } else { nil } }
    private var folder: MailFolder? {
        guard case .folder(let id) = route else { return nil }
        return store.overview?.folders.first { $0.id == id }
    }
    private var searching: Bool { !search.trimmingCharacters(in: .whitespaces).isEmpty }
    private var shown: [MailThread] { threads.filter { !store.removing.contains($0.id) } }
    private var title: String {
        if let box { return box.label }
        return folder?.name ?? "Folder"
    }

    var body: some View {
        List {
            if let folder, !searching {
                Section {
                    FolderHeader(folder: folder)
                }
            }
            ForEach(store.problems, id: \.id) { p in
                Label {
                    Text("\(p.name): \(p.detail)")
                } icon: {
                    Image(systemName: "exclamationmark.circle.fill")
                }
                .font(.subheadline)
                .foregroundStyle(Color.danger)
                .listRowBackground(Color.dangerSoft)
            }
            Section {
                ForEach(shown) { t in
                    NavigationLink(value: MailRoute.thread(t.id)) {
                        MailThreadRow(
                            thread: t,
                            showCategory: searching || box == .inbox,
                            showAddress: showAddress(t)
                        )
                    }
                    .swipeActions(edge: .leading) {
                        Button {
                            Task { await store.markRead(t.id, read: t.unread, api: model.api) }
                        } label: {
                            Label(t.unread ? "Read" : "Unread", systemImage: t.unread ? "envelope.open" : "envelope.badge")
                        }
                        .tint(Color.networkTone)
                    }
                    .swipeActions(edge: .trailing) {
                        Button("Delete", systemImage: "trash", role: .destructive) { deleting = t }
                        if box != .archive {
                            Button("Archive", systemImage: "archivebox") {
                                Task { await store.archive(t.id, api: model.api) }
                            }
                            .tint(Color(hex: 0x8E6AD8))
                        }
                    }
                    .contextMenu { menu(for: t) }
                    .onAppear {
                        if t.id == shown.last?.id { Task { await loadMore() } }
                    }
                }
                if loadingMore {
                    HStack { Spacer(); ProgressView(); Spacer() }
                        .listRowBackground(Color.clear)
                }
            }
        }
        .listStyle(.plain)
        .overlay { emptyState }
        .navigationTitle(searching ? "Search" : title)
        .navigationSubtitle(subtitle)
        .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .automatic), prompt: "Search all mail")
        .refreshable {
            try? await model.api?.refreshMail()
            await reload()
        }
        .task(id: "\(route)-\(search)-\(store.scope)-\(model.revision("mail_changed"))-\(store.localRevision)") {
            if searching { try? await Task.sleep(for: .milliseconds(250)) }
            guard !Task.isCancelled else { return }
            await reload()
        }
        .onAppear { store.remember(route) }
        .toolbar {
            if let folder {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button("Edit", systemImage: "pencil") { editingFolder = folder }
                        Button("Delete folder…", systemImage: "trash", role: .destructive) { deletingFolder = folder }
                    } label: {
                        Image(systemName: "ellipsis")
                    }
                    .accessibilityLabel("Folder options")
                }
            }
            ToolbarSpacer(.flexible, placement: .bottomBar)
            ToolbarItem(placement: .bottomBar) {
                Button("New message", systemImage: "square.and.pencil") {
                    store.compose = MailComposeRequest(draft: MailDraft())
                }
            }
        }
        .sheet(item: $editingFolder) { f in
            if let o = store.overview { MailFolderEditor(folder: f, overview: o) }
        }
        .confirmationDialog(
            "Delete this conversation?",
            isPresented: .constant(deleting != nil),
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                guard let t = deleting else { return }
                deleting = nil
                Task { await store.delete(t.id, api: model.api) }
            }
            Button("Cancel", role: .cancel) { deleting = nil }
        } message: {
            Text("“\(MailText.subject(deleting?.subject ?? ""))” moves to the Trash in your mail account, where you can still get it back from your usual mail app.")
        }
        .confirmationDialog(
            "Delete “\(deletingFolder?.name ?? "")”?",
            isPresented: .constant(deletingFolder != nil),
            titleVisibility: .visible
        ) {
            Button("Delete folder", role: .destructive) {
                guard let f = deletingFolder else { return }
                deletingFolder = nil
                Task {
                    do {
                        try await model.api?.deleteMailFolder(f.id)
                        store.say("“\(f.name)” deleted")
                        store.path.removeAll { $0 == .folder(f.id) }
                        store.changed()
                    } catch {
                        store.error = error.localizedDescription
                    }
                }
            }
            Button("Cancel", role: .cancel) { deletingFolder = nil }
        } message: {
            Text("The folder goes away; the mail in it stays where it is.")
        }
    }

    private var subtitle: String {
        if searching { return "All mail" }
        if let label = store.scopeLabel { return "Received on \(label)" }
        return ""
    }

    private func showAddress(_ t: MailThread) -> Bool {
        guard let o = store.overview, o.manyAddresses, store.scope.address == nil, let on = t.receivedOn else { return false }
        return !o.mainAddresses.contains(on.lowercased()) && !t.lastFromMe
    }

    @ViewBuilder private var emptyState: some View {
        if loaded && shown.isEmpty {
            if searching {
                ContentUnavailableView.search(text: search)
            } else if store.checking {
                ContentUnavailableView {
                    Label { Text("Fetching your mail…") } icon: { ProgressView().controlSize(.large) }
                }
            } else if let folder {
                ContentUnavailableView {
                    Label(folder.toCheck > 0 ? "Filing your mail…" : "Nothing in this folder yet",
                          systemImage: MailFolderLooks.symbol(folder.icon))
                } description: {
                    Text(folder.toCheck > 0 ? "Conversations appear here as they're checked." : "Conversations that fit appear here as they arrive.")
                }
            } else if let box {
                ContentUnavailableView(box.empty, systemImage: box.symbol)
            }
        } else if !loaded {
            ProgressView()
        }
    }

    @ViewBuilder
    private func menu(for t: MailThread) -> some View {
        Button("Reply", systemImage: "arrowshape.turn.up.left") {
            Task { await store.start(.reply, to: t.id, api: model.api) }
        }
        Button("Forward", systemImage: "arrowshape.turn.up.right") {
            Task { await store.start(.forward, to: t.id, api: model.api) }
        }
        Divider()
        Button(t.unread ? "Mark as read" : "Mark as unread", systemImage: t.unread ? "envelope.open" : "envelope.badge") {
            Task { await store.markRead(t.id, read: t.unread, api: model.api) }
        }
        MailFolderMenu(thread: t.id, ids: t.folders, current: folder)
        if box != .archive {
            Button("Archive", systemImage: "archivebox") {
                Task { await store.archive(t.id, api: model.api) }
            }
        }
        Button("Ask \(model.assistantName) about this", systemImage: "sparkles") {
            model.ask(about: .mailThread, id: String(t.id), label: MailText.label(t.subject))
        }
        Divider()
        Button("Delete…", systemImage: "trash", role: .destructive) { deleting = t }
    }

    private func reload() async {
        guard let api = model.api else { return }
        do {
            let q = search
            let list = try await api.mailThreads(
                view: searching ? nil : box,
                query: q,
                scope: store.scope,
                folder: searching ? nil : folderId,
                limit: Self.page
            )
            threads = list
            more = list.count >= Self.page
            loaded = true
        } catch is CancellationError {
        } catch {
            loaded = true
            if threads.isEmpty { store.error = error.localizedDescription }
        }
    }

    private var folderId: Int64? { if case .folder(let id) = route { id } else { nil } }

    private func loadMore() async {
        guard more, !loadingMore, let api = model.api, let last = threads.last else { return }
        loadingMore = true
        defer { loadingMore = false }
        do {
            let page = try await api.mailThreads(
                view: searching ? nil : box,
                query: search,
                scope: store.scope,
                folder: searching ? nil : folderId,
                before: last.lastAt,
                limit: Self.page
            )
            let known = Set(threads.map(\.id))
            threads.append(contentsOf: page.filter { !known.contains($0.id) })
            more = page.count >= Self.page
        } catch {
            more = false
        }
    }
}

/// A smart folder's description and filing progress, above its conversations.
private struct FolderHeader: View {
    let folder: MailFolder

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            MailFolderTile(icon: folder.icon, color: folder.color, size: 36)
            VStack(alignment: .leading, spacing: 4) {
                Text(folder.description)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
                if folder.toCheck > 0 {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.mini)
                        Text("Filing your mail… \(folder.toCheck) to go")
                    }
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 4)
        .listRowSeparator(.hidden)
    }
}

/// "Add to folder ▸" and "Remove from …" in a conversation's menus.
struct MailFolderMenu: View {
    let thread: Int64
    let ids: [Int64]
    /// The folder being looked at, if any: "Remove from" it comes first.
    var current: MailFolder?
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store

    var body: some View {
        let folders = store.overview?.folders ?? []
        let addable = folders.filter { !ids.contains($0.id) }
        let inside = folders.filter { ids.contains($0.id) }
        if !addable.isEmpty {
            Menu {
                ForEach(addable) { f in
                    Button {
                        Task { await store.move(thread, folder: f, member: true, api: model.api) }
                    } label: {
                        Label(f.name, systemImage: MailFolderLooks.symbol(f.icon))
                    }
                }
            } label: {
                Label("Add to folder", systemImage: "folder.badge.plus")
            }
        }
        ForEach(inside.sorted { a, _ in a.id == current?.id }) { f in
            Button {
                Task { await store.move(thread, folder: f, member: false, api: model.api) }
            } label: {
                Label("Remove from “\(f.name)”", systemImage: "folder.badge.minus")
            }
        }
    }
}

/// One conversation in a list, laid out like Mail: who and when, the subject with its
/// tags, then the assistant's one-line summary (or the start of the latest message).
struct MailThreadRow: View {
    let thread: MailThread
    var showCategory = false
    var showAddress = false

    var body: some View {
        let t = thread
        HStack(alignment: .top, spacing: 8) {
            Circle()
                .fill(t.unread ? Color(hex: 0x0A84FF) : .clear)
                .frame(width: 9, height: 9)
                .padding(.top, 6)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    whoLine.lineLimit(1)
                    Spacer(minLength: 4)
                    Text(MailText.listDate(t.lastAt))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
                HStack(spacing: 6) {
                    Text(MailText.subject(t.subject))
                        .font(.subheadline)
                        .lineLimit(1)
                    if t.flagged {
                        Image(systemName: "star.fill").font(.caption2).foregroundStyle(Color(hex: 0xFF9F0A))
                            .accessibilityLabel("Flagged")
                    }
                    Spacer(minLength: 0)
                    if showAddress, let on = t.receivedOn {
                        MailPill(text: on, fg: .secondary, bg: Color(hex: 0x767680, opacity: 0.12))
                            .frame(maxWidth: 130, alignment: .trailing)
                    }
                    if let pill { MailPill(text: pill.0, fg: pill.1, bg: pill.2) }
                }
                summaryLine
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 3)
        .accessibilityElement(children: .combine)
    }

    private var whoLine: Text {
        let who = Text(MailText.who(thread)).fontWeight(thread.unread ? .semibold : .medium)
        guard thread.messageCount > 1 else { return who }
        return Text("\(who)  \(Text("\(thread.messageCount)").foregroundStyle(.tertiary))")
    }

    private var summaryLine: Text {
        if let s = thread.summary, !s.isEmpty {
            return Text("\(Text(Image(systemName: "sparkles")).foregroundStyle(.tertiary)) \(s)")
        }
        return Text(verbatim: thread.snippet)
    }

    private var pill: (String, Color, Color)? {
        if thread.suspicious { return ("Suspicious", .danger, .dangerSoft) }
        guard showCategory else { return nil }
        switch thread.category {
        case .needsReply: return ("Reply", .cloudTone, .cloudSoft)
        case .important: return ("Important", .networkTone, .networkSoft)
        default: return nil
        }
    }
}

/// A small capsule label.
struct MailPill: View {
    let text: String
    let fg: Color
    let bg: Color

    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .lineLimit(1)
            .truncationMode(.middle)
            .foregroundStyle(fg)
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(bg, in: .capsule)
    }
}
