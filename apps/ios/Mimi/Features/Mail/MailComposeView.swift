import SwiftUI

/// A message being written, laid out like Mail's compose sheet: From (when there's a
/// choice), To, Cc and Subject over the text. Replies can be drafted by the assistant
/// first. Nothing is sent until the user taps Send: that tap is the approval.
struct MailComposeView: View {
    let request: MailComposeRequest
    /// Called with a short confirmation once it's sent.
    var onSent: (String) -> Void = { _ in }
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var draft: MailDraft
    @State private var to: String
    @State private var cc: String
    @State private var showCc: Bool
    @State private var overview: MailOverview?
    @State private var instructions = ""
    @State private var writing = false
    @State private var sending = false
    @State private var confirmDiscard = false
    @State private var error: String?
    @State private var autoDrafted = false
    @FocusState private var focus: Field?

    private enum Field { case to, cc, subject, body, instructions }

    init(request: MailComposeRequest, onSent: @escaping (String) -> Void = { _ in }) {
        self.request = request
        self.onSent = onSent
        _draft = State(initialValue: request.draft)
        _to = State(initialValue: MailText.joinAddresses(request.draft.to))
        _cc = State(initialValue: MailText.joinAddresses(request.draft.cc))
        _showCc = State(initialValue: !request.draft.cc.isEmpty)
    }

    private var title: String {
        if request.draft.replyTo != nil { return "Reply" }
        if request.draft.subject.hasPrefix("Fwd:") { return "Forward" }
        return "New message"
    }

    private var edited: Bool { draft != request.draft }
    private var canSend: Bool {
        !draft.to.isEmpty && !draft.body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !sending && !writing
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let thread = request.thread {
                        assistantBar(thread)
                    }
                    fromLine
                    line("To") {
                        TextField("", text: $to, axis: .vertical)
                            .keyboardType(.emailAddress)
                            .textContentType(.emailAddress)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .focused($focus, equals: .to)
                            .accessibilityLabel("To")
                            .accessibilityIdentifier("compose-to")
                            .onChange(of: to) { _, v in draft.to = MailText.splitAddresses(v) }
                        if !showCc {
                            Button("Cc") {
                                showCc = true
                                focus = .cc
                            }
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                        }
                    }
                    if showCc {
                        line("Cc") {
                            TextField("", text: $cc, axis: .vertical)
                                .keyboardType(.emailAddress)
                                .textContentType(.emailAddress)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                                .focused($focus, equals: .cc)
                                .accessibilityLabel("Cc")
                                .onChange(of: cc) { _, v in draft.cc = MailText.splitAddresses(v) }
                        }
                    }
                    line("Subject") {
                        TextField("", text: $draft.subject, axis: .vertical)
                            .font(.body.weight(.medium))
                            .focused($focus, equals: .subject)
                            .accessibilityLabel("Subject")
                    }
                    if draft.forwardOf != nil {
                        Label("The original's attachments go along.", systemImage: "paperclip")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 20)
                            .padding(.vertical, 10)
                    }
                    TextField("Write your message", text: $draft.body, axis: .vertical)
                        .lineLimit(12...)
                        .lineSpacing(2)
                        .focused($focus, equals: .body)
                        .padding(.horizontal, 20)
                        .padding(.vertical, 14)
                        .accessibilityLabel("Message")
                        .accessibilityIdentifier("compose-body")
                        .overlay(alignment: .topLeading) {
                            if writing {
                                Label("Writing…", systemImage: "sparkles")
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)
                                    .shimmering()
                                    .padding(.horizontal, 20)
                                    .padding(.vertical, 14)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(Color(uiColor: .systemBackground))
                            }
                        }
                }
            }
            .scrollDismissesKeyboard(.interactively)
            .background(Color(uiColor: .systemBackground))
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .safeAreaInset(edge: .bottom) {
                Text("Sent from your account when you tap Send.")
                    .font(.footnote)
                    .foregroundStyle(.tertiary)
                    .frame(maxWidth: .infinity)
                    .padding(.bottom, 6)
                    .opacity(focus == nil ? 1 : 0)
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", systemImage: "xmark") {
                        if edited { confirmDiscard = true } else { close() }
                    }
                    .disabled(sending)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        Task { await send() }
                    } label: {
                        if sending {
                            ProgressView()
                        } else {
                            Image(systemName: "arrow.up")
                                .font(.body.weight(.bold))
                        }
                    }
                    .buttonStyle(.glassProminent)
                    .tint(Color.lime)
                    .foregroundStyle(Color.limeInk)
                    .disabled(!canSend)
                    .accessibilityLabel("Send")
                    .accessibilityIdentifier("compose-send")
                }
            }
            .confirmationDialog("Discard this message?", isPresented: $confirmDiscard, titleVisibility: .visible) {
                Button("Discard", role: .destructive) { close() }
                Button("Keep writing", role: .cancel) {}
            }
            .alert("Not sent", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
        .interactiveDismissDisabled(edited || sending)
        .task {
            if overview == nil { overview = try? await model.api?.mailOverview() }
        }
        .task {
            if request.autoDraft, let thread = request.thread, !autoDrafted {
                autoDrafted = true
                await write(thread)
            } else if draft.to.isEmpty {
                focus = .to
            } else if draft.body.isEmpty && request.thread == nil {
                focus = .body
            }
        }
    }

    // MARK: Pieces

    private func line<Content: View>(_ label: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(label)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .frame(width: 62, alignment: .leading)
                content()
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            Divider().padding(.leading, 20)
        }
    }

    /// Which address it's sent from, when there's a choice: every connected account and
    /// the aliases mail has arrived at. Replies start from the address mail was sent to.
    @ViewBuilder private var fromLine: some View {
        if let o = overview, o.sendingAddresses.count > 1 {
            let account = o.accounts.first { $0.connectionId == draft.connectionId } ?? o.accounts.first
            let current = (draft.from ?? account?.email ?? "").lowercased()
            line("From") {
                Menu {
                    ForEach(o.sendingAddresses, id: \.email) { option in
                        Button {
                            draft.connectionId = option.account
                            draft.from = option.email
                        } label: {
                            if option.email.lowercased() == current {
                                Label(option.email, systemImage: "checkmark")
                            } else {
                                Text(option.email)
                            }
                        }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text(current).lineLimit(1).truncationMode(.middle)
                        Image(systemName: "chevron.up.chevron.down").font(.caption2.weight(.semibold))
                    }
                    .foregroundStyle(Color.ink)
                }
                .accessibilityLabel("From \(current)")
                Spacer(minLength: 0)
            }
        }
    }

    /// "What should it say?" and "Draft it for me", for replies.
    private func assistantBar(_ thread: Int64) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: "sparkles").foregroundStyle(.secondary)
                TextField("What should it say? (optional)", text: $instructions, axis: .vertical)
                    .font(.callout)
                    .focused($focus, equals: .instructions)
                    .submitLabel(.go)
                    .onSubmit { Task { await write(thread) } }
            }
            HStack(spacing: 8) {
                Button {
                    Task { await write(thread) }
                } label: {
                    HStack(spacing: 6) {
                        if writing { ProgressView().controlSize(.small) }
                        Text(draft.body.isEmpty ? "Draft it for me" : "Write again")
                    }
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .controlSize(.small)
                .disabled(writing || sending)
                if let locality = overview?.modelLocality {
                    LocalityBadge(locality: locality)
                }
                Spacer(minLength: 0)
            }
        }
        .padding(14)
        .background(Color.subtle, in: .rect(cornerRadius: 16))
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    // MARK: Actions

    private func write(_ thread: Int64) async {
        guard let api = model.api else { return }
        writing = true
        focus = nil
        defer { writing = false }
        do {
            let i = instructions.trimmingCharacters(in: .whitespacesAndNewlines)
            let d = try await api.draftMailReply(thread, instructions: i.isEmpty ? nil : i)
            withAnimation {
                draft.body = d.body
                if draft.to.isEmpty {
                    draft.to = d.to
                    to = MailText.joinAddresses(d.to)
                }
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func send() async {
        guard let api = model.api else { return }
        if draft.to.isEmpty {
            error = "Add at least one recipient."
            return
        }
        if let bad = (draft.to + draft.cc).first(where: { !MailText.looksLikeAddress($0) }) {
            error = "“\(bad)” doesn't look like an email address."
            return
        }
        sending = true
        defer { sending = false }
        do {
            try await api.sendMail(draft)
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            onSent(draft.to.count == 1 ? "Sent to \(draft.to[0])" : "Sent")
            request.onSent?()
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func close() {
        request.onClose?(draft)
        dismiss()
    }
}
