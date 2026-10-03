import SwiftUI

/// One conversation: its subject and folders, a warning when it's suspicious, the
/// assistant's summary, every message (older ones folded), and Reply. Archive, Delete,
/// Reply and a new message are in the bottom bar; the rest is in the "…" menu.
struct MailReaderView: View {
    let id: Int64
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store
    @AppStorage(MailMode.key) private var mode: MailMode = .original
    @State private var detail: MailThreadDetail?
    @State private var gone = false
    @State private var summary: String?
    @State private var summarizing = false
    @State private var confirmDelete = false
    @State private var markedRead = false
    @State private var open: Set<Int64> = []

    var body: some View {
        Group {
            if let d = detail {
                content(d)
            } else if gone {
                ContentUnavailableView("This conversation isn't here any more", systemImage: "tray")
            } else {
                ProgressView()
            }
        }
        .background(Color.canvas)
        .navigationBarTitleDisplayMode(.inline)
        // The conversation's own bar (Archive, Delete, Reply) takes the bottom, as in Mail.
        .toolbar(.hidden, for: .tabBar)
        .task(id: "\(id)-\(model.revision("mail_changed"))-\(store.localRevision)") { await load() }
        .toolbar { if let d = detail { toolbar(d) } }
        .confirmationDialog("Delete this conversation?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                Task {
                    await store.delete(id, api: model.api)
                    leave()
                }
            }
        } message: {
            Text("“\(MailText.subject(detail?.thread.subject ?? ""))” moves to the Trash in your mail account, where you can still get it back from your usual mail app.")
        }
    }

    // MARK: Content

    private func content(_ d: MailThreadDetail) -> some View {
        let t = d.thread
        return ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(MailText.subject(t.subject))
                        .font(.title2.weight(.semibold))
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                    if store.overview?.manyAddresses == true, let on = t.receivedOn {
                        Label("Received on \(on)", systemImage: "at")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    FolderChips(thread: t.id, ids: t.folders)
                }
                MailModePicker(original: d.messages.contains { $0.hasHtml != false }, mode: $mode)

                if t.suspicious { SuspiciousNote() }

                if let s = summary ?? t.summary, !s.isEmpty {
                    SummaryCard(text: s, locality: store.overview?.modelLocality)
                        .transition(.opacity.combined(with: .move(edge: .top)))
                }

                ForEach(d.messages) { m in
                    if open.contains(m.id) {
                        MailMessageCard(message: m, mode: effectiveMode(d), detail: d)
                            .transition(.opacity)
                    } else {
                        FoldedMessage(message: m) {
                            withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { _ = open.insert(m.id) }
                        }
                    }
                }

                Button {
                    store.compose = MailComposeRequest(draft: MailText.reply(to: d), thread: t.id)
                } label: {
                    Label("Reply to \(t.participants.first?.shortName ?? "this conversation")…", systemImage: "arrowshape.turn.up.left")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 14)
                        .background(.background, in: .rect(cornerRadius: 18))
                        .shadow(color: .black.opacity(0.05), radius: 6, y: 2)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("reply-box")
            }
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 24)
            .animation(.spring(response: 0.4, dampingFraction: 0.85), value: summary)
        }
    }

    /// Original only when some message has it; Formatted instead.
    private func effectiveMode(_ d: MailThreadDetail) -> MailMode {
        mode == .original && !d.messages.contains(where: { $0.hasHtml != false }) ? .formatted : mode
    }

    // MARK: Toolbar

    @ToolbarContentBuilder
    private func toolbar(_ d: MailThreadDetail) -> some ToolbarContent {
        let t = d.thread
        let replyAll = MailText.replyAll(to: d, mine: store.overview?.ownAddresses ?? [])
        ToolbarItem(placement: .topBarTrailing) {
            Menu {
                Button(summarizing ? "Summarizing…" : "Summarize", systemImage: "sparkles") { Task { await summarize() } }
                    .disabled(summarizing)
                Button("Draft a reply with \(model.assistantName)", systemImage: "wand.and.sparkles") {
                    store.compose = MailComposeRequest(draft: MailText.reply(to: d), thread: t.id, autoDraft: true)
                }
                Button("Ask \(model.assistantName) about this", systemImage: "bubble.left.and.text.bubble.right") {
                    model.ask(about: .mailThread, id: String(t.id), label: MailText.label(t.subject))
                }
                Divider()
                Button("Mark as unread", systemImage: "envelope.badge") {
                    Task {
                        await store.markRead(id, read: false, api: model.api)
                        leave()
                    }
                }
                MailFolderMenu(thread: t.id, ids: t.folders)
                Divider()
                Button("Delete…", systemImage: "trash", role: .destructive) { confirmDelete = true }
            } label: {
                Image(systemName: "ellipsis")
            }
            .accessibilityLabel("More")
        }
        ToolbarItem(placement: .bottomBar) {
            Button("Archive", systemImage: "archivebox") {
                Task {
                    await store.archive(id, api: model.api)
                    leave()
                }
            }
        }
        ToolbarItem(placement: .bottomBar) {
            Button("Delete", systemImage: "trash") { confirmDelete = true }
        }
        ToolbarSpacer(.flexible, placement: .bottomBar)
        ToolbarItem(placement: .bottomBar) {
            Menu {
                Button("Reply", systemImage: "arrowshape.turn.up.left") {
                    store.compose = MailComposeRequest(draft: MailText.reply(to: d), thread: t.id)
                }
                if let replyAll {
                    Button("Reply all", systemImage: "arrowshape.turn.up.left.2") {
                        store.compose = MailComposeRequest(draft: replyAll, thread: t.id)
                    }
                }
                Button("Forward", systemImage: "arrowshape.turn.up.right") {
                    if let f = MailText.forward(d) { store.compose = MailComposeRequest(draft: f) }
                }
            } label: {
                Label("Reply", systemImage: "arrowshape.turn.up.left")
            } primaryAction: {
                store.compose = MailComposeRequest(draft: MailText.reply(to: d), thread: t.id)
            }
        }
        ToolbarItem(placement: .bottomBar) {
            Button("New message", systemImage: "square.and.pencil") {
                store.compose = MailComposeRequest(draft: MailDraft())
            }
        }
    }

    // MARK: Actions

    private func load() async {
        guard let api = model.api else { return }
        do {
            let d = try await api.mailThread(id)
            if detail == nil {
                // Long threads start with the older messages folded, like mail apps.
                open = Set(d.messages.suffix(2).map(\.id))
            } else {
                // New messages arrive open.
                let known = Set(detail?.messages.map(\.id) ?? [])
                for m in d.messages where !known.contains(m.id) { open.insert(m.id) }
            }
            detail = d
            // Opening an unread conversation marks it read, as mail apps do.
            if d.thread.unread && !markedRead {
                markedRead = true
                try? await api.markMailRead(id, read: true)
            }
        } catch is CancellationError {
        } catch MimiError.api(let code, _) where code == "not_found" {
            gone = true
            detail = nil
        } catch {
            if detail == nil { store.error = error.localizedDescription }
        }
    }

    private func summarize() async {
        guard let api = model.api else { return }
        summarizing = true
        defer { summarizing = false }
        do {
            summary = try await api.summarizeMail(id)
        } catch {
            store.error = error.localizedDescription
        }
    }

    /// Back to the list (the conversation was archived, deleted or marked unread).
    private func leave() {
        if store.path.last == .thread(id) { store.path.removeLast() }
    }
}

// MARK: - Pieces of the reader

/// Text · Formatted · Original, shared by every message and remembered on this phone.
enum MailMode: String, CaseIterable, Identifiable {
    case text, formatted, original
    static let key = "mimi.mail.mode"
    var id: String { rawValue }
    var label: String {
        switch self {
        case .text: "Text"
        case .formatted: "Formatted"
        case .original: "Original"
        }
    }
}

struct MailModePicker: View {
    let original: Bool
    @Binding var mode: MailMode

    var body: some View {
        let modes = original ? MailMode.allCases : [.text, .formatted]
        let current = !original && mode == .original ? MailMode.formatted : mode
        Picker("Show emails as", selection: Binding(get: { current }, set: { mode = $0 })) {
            ForEach(modes) { Text($0.label).tag($0) }
        }
        .pickerStyle(.segmented)
        .accessibilityIdentifier("mail-mode")
    }
}

private struct SuspiciousNote: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "exclamationmark.shield.fill")
                .foregroundStyle(Color.danger)
                .font(.title3)
            VStack(alignment: .leading, spacing: 4) {
                Text("This email looks suspicious")
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(Color.danger)
                Text("It contains instructions written for AI assistants, a trick used to make them share or send your mail. \(model.assistantName) won't follow them, and nothing is ever sent without your OK. Be careful with any link or request in it.")
                    .font(.subheadline)
                    .foregroundStyle(Color.ink.opacity(0.8))
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.dangerSoft, in: .rect(cornerRadius: 16))
        .accessibilityElement(children: .combine)
    }
}

private struct SummaryCard: View {
    let text: String
    let locality: Locality?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "sparkles").foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 8) {
                Text(text)
                    .font(.callout)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                if let locality {
                    HStack(spacing: 6) {
                        Text("Written by your model").font(.footnote).foregroundStyle(.secondary)
                        LocalityBadge(locality: locality)
                    }
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.subtle, in: .rect(cornerRadius: 16))
    }
}

/// The folders a conversation is in, each removable.
private struct FolderChips: View {
    let thread: Int64
    let ids: [Int64]
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store

    var body: some View {
        let shown = (store.overview?.folders ?? []).filter { ids.contains($0.id) }
        if !shown.isEmpty {
            FlowLayout(spacing: 6, lineSpacing: 6) {
                ForEach(shown) { f in
                    HStack(spacing: 4) {
                        Image(systemName: MailFolderLooks.symbol(f.icon)).font(.caption2.weight(.semibold))
                        Text(f.name).font(.caption.weight(.medium))
                        Button {
                            Task { await store.move(thread, folder: f, member: false, api: model.api) }
                        } label: {
                            Image(systemName: "xmark").font(.caption2.weight(.bold))
                                .frame(width: 18, height: 18)
                                .contentShape(.circle)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Remove from \(f.name)")
                    }
                    .foregroundStyle(MailFolderLooks.ink(f.color))
                    .padding(.leading, 9)
                    .padding(.trailing, 3)
                    .padding(.vertical, 3)
                    .background(MailFolderLooks.soft(f.color), in: .capsule)
                }
            }
        }
    }
}

/// An older message, folded to one line: who, the start, when.
private struct FoldedMessage: View {
    let message: MailMessage
    let unfold: () -> Void

    var body: some View {
        Button(action: unfold) {
            HStack(spacing: 10) {
                Text(message.fromMe ? "You" : message.from.shortName)
                    .font(.callout.weight(.medium))
                    .lineLimit(1)
                    .frame(maxWidth: 110, alignment: .leading)
                Text(verbatim: String(message.body.prefix(200)).replacingOccurrences(of: "\n", with: " "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(MailText.listDate(message.date))
                    .font(.footnote)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 13)
            .background(.background, in: .rect(cornerRadius: 18))
            .shadow(color: .black.opacity(0.05), radius: 6, y: 2)
        }
        .buttonStyle(.plain)
        .accessibilityHint("Shows this message")
    }
}

/// One message: who sent it to whom and when, its body in the chosen mode, and its
/// attachments. Long-press for Reply, Forward, Copy and Ask.
struct MailMessageCard: View {
    let message: MailMessage
    let mode: MailMode
    let detail: MailThreadDetail
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store

    var body: some View {
        let m = message
        let recipients = m.to + m.cc
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        if m.fromMe {
                            Text("You").font(.callout.weight(.semibold))
                        } else {
                            Menu {
                                Text(m.from.email)
                                Button("New message", systemImage: "square.and.pencil") {
                                    store.compose = MailComposeRequest(draft: MailDraft(connectionId: detail.thread.connectionId, to: [m.from.email]))
                                }
                                Button("Copy address", systemImage: "doc.on.doc") { UIPasteboard.general.string = m.from.email }
                            } label: {
                                Text(m.from.shortName).font(.callout.weight(.semibold)).foregroundStyle(Color.ink)
                            }
                        }
                        if !m.fromMe, m.from.name != nil {
                            Text(m.from.email).font(.footnote).foregroundStyle(.tertiary).lineLimit(1).truncationMode(.middle)
                        }
                    }
                    if !recipients.isEmpty {
                        Text("to \(recipients.map(\.shortName).joined(separator: ", "))")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 4)
                Text(MailText.longDate(m.date))
                    .font(.footnote)
                    .foregroundStyle(.tertiary)
                    .multilineTextAlignment(.trailing)
            }
            MailMessageBody(message: m, mode: mode) { address in
                store.compose = MailComposeRequest(draft: MailDraft(connectionId: detail.thread.connectionId, to: [address]))
            }
            if !m.attachments.isEmpty {
                MailAttachments(message: m)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background, in: .rect(cornerRadius: 18))
        .shadow(color: .black.opacity(0.06), radius: 10, y: 3)
        .contextMenu {
            Button("Reply", systemImage: "arrowshape.turn.up.left") {
                store.compose = MailComposeRequest(draft: MailText.reply(to: detail), thread: detail.thread.id)
            }
            if let all = MailText.replyAll(to: detail, mine: store.overview?.ownAddresses ?? []) {
                Button("Reply all", systemImage: "arrowshape.turn.up.left.2") {
                    store.compose = MailComposeRequest(draft: all, thread: detail.thread.id)
                }
            }
            Button("Forward", systemImage: "arrowshape.turn.up.right") {
                if let f = MailText.forward(detail, message: m) { store.compose = MailComposeRequest(draft: f) }
            }
            Divider()
            Button("Copy message text", systemImage: "doc.on.doc") { UIPasteboard.general.string = m.body }
            Button("Ask \(model.assistantName) about this conversation", systemImage: "sparkles") {
                model.ask(about: .mailThread, id: String(detail.thread.id), label: MailText.label(detail.thread.subject))
            }
        }
    }
}
