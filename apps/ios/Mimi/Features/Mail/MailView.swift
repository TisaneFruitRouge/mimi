import SwiftUI

/// The Mail tab: the mailboxes (views sorted for the user, the usual mailboxes, smart
/// folders, "Received on"), the conversations in one, and one conversation. It opens on
/// the last view, with the mailboxes one step back, as Mail does. Sending is always the
/// user's own tap on a message they can read in full.
struct MailView: View {
    @Environment(AppModel.self) private var model
    @State private var store = MailStore()

    var body: some View {
        @Bindable var store = store
        NavigationStack(path: $store.path) {
            MailboxesView()
                .navigationDestination(for: MailRoute.self) { route in
                    switch route {
                    case .box, .folder: MailListView(route: route)
                    case .thread(let id): MailReaderView(id: id)
                    }
                }
        }
        .task(id: "\(model.revision("mail_changed"))-\(model.revision("connections_changed"))-\(store.localRevision)-\(model.phase == .online)") {
            await store.load(model.api)
        }
        .onAppear { MailStore.cleanAttachmentsOnce() }
        .sheet(item: $store.compose) { request in
            MailComposeView(request: request) { text in
                store.say(text)
                store.changed()
            }
        }
        .overlay(alignment: .bottom) { MailToast(text: store.toast) }
        .alert("Something went wrong", isPresented: .constant(store.error != nil)) {
            Button("OK") { store.error = nil }
        } message: {
            Text(store.error ?? "")
        }
        .environment(store)
    }
}

/// "Archived", "Sent to sam@example.com": a moment above the tab bar.
struct MailToast: View {
    let text: String?

    var body: some View {
        Group {
            if let text {
                Label(text, systemImage: "checkmark.circle.fill")
                    .font(.subheadline.weight(.medium))
                    .labelStyle(ToastLabel())
                    .padding(.horizontal, 16)
                    .padding(.vertical, 11)
                    .glassEffect(.regular, in: .capsule)
                    .padding(.bottom, 64)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: text)
    }

    private struct ToastLabel: LabelStyle {
        func makeBody(configuration: Configuration) -> some View {
            HStack(spacing: 8) {
                configuration.icon.foregroundStyle(Color.privateTone)
                configuration.title
            }
        }
    }
}

// MARK: - The mailboxes

struct MailboxesView: View {
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store
    @State private var editingFolder: MailFolder?
    @State private var creatingFolder = false
    @State private var deletingFolder: MailFolder?

    var body: some View {
        Group {
            if let o = store.overview {
                if o.accounts.isEmpty {
                    NoMailAccount()
                } else {
                    list(o)
                }
            } else if let error = store.loadError {
                ContentUnavailableView {
                    Label("Mail isn't available", systemImage: "envelope.badge.shield.half.filled")
                } description: {
                    Text(error)
                } actions: {
                    Button("Try again") { Task { await store.load(model.api) } }
                }
            } else {
                ProgressView().controlSize(.large)
            }
        }
        .navigationTitle("Mail")
        .toolbar {
            if store.overview?.accounts.isEmpty == false {
                ToolbarSpacer(.flexible, placement: .bottomBar)
                ToolbarItem(placement: .bottomBar) {
                    Button("New message", systemImage: "square.and.pencil") {
                        store.compose = MailComposeRequest(draft: MailDraft())
                    }
                }
            }
        }
        .sheet(isPresented: $creatingFolder) {
            if let o = store.overview { MailFolderEditor(folder: nil, overview: o) }
        }
        .sheet(item: $editingFolder) { f in
            if let o = store.overview { MailFolderEditor(folder: f, overview: o) }
        }
        .confirmationDialog(
            "Delete “\(deletingFolder?.name ?? "")”?",
            isPresented: .constant(deletingFolder != nil),
            titleVisibility: .visible
        ) {
            Button("Delete folder", role: .destructive) {
                guard let f = deletingFolder else { return }
                deletingFolder = nil
                Task { await deleteFolder(f) }
            }
            Button("Cancel", role: .cancel) { deletingFolder = nil }
        } message: {
            Text("The folder goes away; the mail in it stays where it is.")
        }
    }

    private func list(_ o: MailOverview) -> some View {
        let rows = MailStore.scopeRows(o)
        return List {
            if o.sorting {
                Section("Sorted for you") {
                    ForEach(MailBox.allCases.filter(\.sorted)) { box in
                        boxRow(box, count: box == .needsReply ? o.needsReply : box == .important ? o.important : 0)
                    }
                }
            }
            Section("Mailboxes") {
                ForEach(MailBox.allCases.filter { !$0.sorted }) { box in
                    boxRow(box, count: box == .inbox ? o.unread : 0)
                }
            }
            Section {
                if o.folders.isEmpty {
                    Button {
                        creatingFolder = true
                    } label: {
                        Label {
                            Text("Make a folder, say what goes in it, and your mail is filed for you.")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        } icon: {
                            Image(systemName: "folder.badge.plus").foregroundStyle(.secondary)
                        }
                    }
                }
                ForEach(o.folders) { f in
                    NavigationLink(value: MailRoute.folder(f.id)) {
                        HStack(spacing: 12) {
                            MailFolderTile(icon: f.icon, color: f.color, size: 28)
                            Text(f.name).lineLimit(1)
                            Spacer(minLength: 8)
                            if f.toCheck > 0 {
                                ProgressView().controlSize(.mini).accessibilityLabel("Filing")
                            } else if f.unread > 0 {
                                Text("\(f.unread)").foregroundStyle(.secondary).monospacedDigit()
                            }
                        }
                    }
                    .swipeActions {
                        Button("Delete", systemImage: "trash", role: .destructive) { deletingFolder = f }
                        Button("Edit", systemImage: "pencil") { editingFolder = f }
                    }
                    .contextMenu {
                        Button("Edit", systemImage: "pencil") { editingFolder = f }
                        Button("Delete folder…", systemImage: "trash", role: .destructive) { deletingFolder = f }
                    }
                }
            } header: {
                HStack {
                    Text("Smart folders")
                    Spacer()
                    Button("New smart folder", systemImage: "plus") { creatingFolder = true }
                        .labelStyle(.iconOnly)
                        .font(.subheadline.weight(.semibold))
                }
            }
            if !rows.isEmpty {
                Section("Received on") {
                    scopeRow(o.accounts.count > 1 ? "All accounts" : "All addresses", icon: "square.stack", scope: .all, unread: nil, nested: false)
                    ForEach(rows) { r in
                        scopeRow(r.label, icon: r.account ? "envelope" : "at", scope: r.scope, unread: r.unread, nested: r.nested)
                    }
                }
            }
            Section {
            } footer: {
                sortingNote(o, single: rows.isEmpty)
            }
        }
        .listStyle(.insetGrouped)
        .refreshable {
            try? await model.api?.refreshMail()
            await store.load(model.api)
        }
    }

    private func boxRow(_ box: MailBox, count: Int) -> some View {
        NavigationLink(value: MailRoute.box(box)) {
            HStack(spacing: 12) {
                Image(systemName: box.symbol)
                    .font(.body)
                    .foregroundStyle(Color.networkTone)
                    .frame(width: 28)
                Text(box.label)
                Spacer(minLength: 8)
                if count > 0 {
                    Text("\(count)").foregroundStyle(.secondary).monospacedDigit()
                }
            }
        }
    }

    private func scopeRow(_ label: String, icon: String, scope: MailScope, unread: Int?, nested: Bool) -> some View {
        Button {
            store.setScope(scope)
        } label: {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .foregroundStyle(.secondary)
                    .frame(width: 28)
                Text(label)
                    .foregroundStyle(Color.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 8)
                if let unread, unread > 0 {
                    Text("\(unread)").foregroundStyle(.secondary).monospacedDigit()
                }
                if store.scope == scope {
                    Image(systemName: "checkmark").font(.body.weight(.semibold)).foregroundStyle(Color.limeDeep)
                }
            }
            .padding(.leading, nested ? 20 : 0)
        }
        .accessibilityAddTraits(store.scope == scope ? .isSelected : [])
    }

    @ViewBuilder
    private func sortingNote(_ o: MailOverview, single: Bool) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if o.sorting {
                if let locality = o.sorterLocality {
                    HStack(spacing: 6) {
                        Text(o.sorter == .jev ? "Sorted by Jev" : "Sorted by your model")
                        LocalityBadge(locality: locality)
                    }
                }
            } else {
                Text("Sorting is off. Turn it on in Settings › Privacy.")
            }
            if single {
                ForEach(o.accounts) { a in Text(a.email) }
            }
        }
        .font(.footnote)
    }

    private func deleteFolder(_ f: MailFolder) async {
        do {
            try await model.api?.deleteMailFolder(f.id)
            store.say("“\(f.name)” deleted")
            store.path.removeAll { $0 == .folder(f.id) }
            await store.load(model.api)
        } catch {
            store.error = error.localizedDescription
        }
    }
}

/// No mailbox yet: what Mail does, and where to connect one.
private struct NoMailAccount: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ContentUnavailableView {
            Label {
                Text("Your email, sorted")
            } icon: {
                Image(systemName: "envelope")
                    .font(.system(size: 30, weight: .medium))
                    .foregroundStyle(MailFolderLooks.ink("violet"))
                    .frame(width: 64, height: 64)
                    .background(MailFolderLooks.soft("violet"), in: .rect(cornerRadius: 18))
            }
        } description: {
            Text("Connect your email and \(model.assistantName) sorts what needs a reply from the rest, sums up long threads and drafts answers. It all happens on your computer, and nothing is sent without your OK.\n\nConnect it in Settings › Connections.")
        } actions: {
            Button("Open Settings") { model.tab = .settings }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
        }
    }
}
