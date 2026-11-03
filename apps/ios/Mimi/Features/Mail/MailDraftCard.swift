import SwiftUI

/// Tools that leave a draft in the chat for the user to check and send.
nonisolated enum MailDrafts {
    static let tools: Set<String> = ["mail_draft_reply", "mail_compose"]

    /// The draft an action left, if it's one of those and it's done.
    static func draft(of action: Action) -> MailDraft? {
        guard tools.contains(action.tool), action.status == .done, let d = action.output?["draft"],
              let data = try? JSONEncoder().encode(d),
              let draft = try? JSONDecoder().decode(MailDraft.self, from: data) else { return nil }
        return draft
    }

    // Drafts sent or put away on this phone, so reopening the chat doesn't offer them again.
    static let sentKey = "mimi.mail.sent-drafts"
    static let discardedKey = "mimi.mail.discarded-drafts"

    static func marked(_ id: UUID, _ key: String) -> Bool {
        (UserDefaults.standard.stringArray(forKey: key) ?? []).contains(id.uuidString)
    }

    static func mark(_ id: UUID, _ key: String, _ on: Bool = true) {
        var list = (UserDefaults.standard.stringArray(forKey: key) ?? []).filter { $0 != id.uuidString }
        if on { list.append(id.uuidString) }
        UserDefaults.standard.set(Array(list.suffix(200)), forKey: key)
    }
}

/// A draft email in the chat: the whole message to check, then Send (the user's tap is
/// the approval), Edit in the compose sheet, or Discard.
struct MailDraftCard: View {
    let action: Action
    @Environment(AppModel.self) private var model
    @State private var draft: MailDraft?
    @State private var sent = false
    @State private var discarded = false
    @State private var sending = false
    @State private var editing: MailComposeRequest?
    @State private var error: String?

    var body: some View {
        Group {
            if let d = draft ?? MailDrafts.draft(of: action) {
                if sent {
                    doneLine(icon: "checkmark", tone: .privateTone, soft: .privateSoft,
                             text: "Sent “\(MailText.subject(d.subject))” to \(d.to.joined(separator: ", "))")
                } else if discarded {
                    HStack(spacing: 10) {
                        doneLine(icon: "xmark", tone: .secondary, soft: .subtle, text: "Draft email put away")
                        Button("Show") {
                            withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) {
                                discarded = false
                                MailDrafts.mark(action.id, MailDrafts.discardedKey, false)
                            }
                        }
                        .font(.subheadline.weight(.medium))
                    }
                } else {
                    card(d)
                        .transition(.asymmetric(insertion: .scale(scale: 0.97).combined(with: .opacity), removal: .opacity))
                }
            }
        }
        .onAppear {
            sent = MailDrafts.marked(action.id, MailDrafts.sentKey)
            discarded = MailDrafts.marked(action.id, MailDrafts.discardedKey)
        }
        .sheet(item: $editing) { request in
            MailComposeView(request: request)
        }
        .alert("Not sent", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    private func card(_ d: MailDraft) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "envelope")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(MailFolderLooks.ink("violet"))
                    .frame(width: 30, height: 30)
                    .background(MailFolderLooks.soft("violet"), in: .rect(cornerRadius: 9))
                Text("Draft email · not sent")
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 16)
            .padding(.top, 14)
            .padding(.bottom, 10)
            Divider()
            field("To", d.to.isEmpty ? "(nobody yet)" : d.to.joined(separator: ", "))
            if !d.cc.isEmpty { field("Cc", d.cc.joined(separator: ", ")) }
            field("Subject", MailText.subject(d.subject), weight: .medium)
            Text(verbatim: d.body.isEmpty ? " " : d.body)
                .font(.callout)
                .lineSpacing(2)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
            Divider()
            HStack(spacing: 8) {
                Menu {
                    Button("Discard", systemImage: "trash", role: .destructive) {
                        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) {
                            discarded = true
                            MailDrafts.mark(action.id, MailDrafts.discardedKey)
                        }
                    }
                } label: {
                    Image(systemName: "ellipsis")
                        .frame(width: 36, height: 36)
                        .contentShape(.circle)
                }
                .foregroundStyle(.secondary)
                .accessibilityLabel("More")
                Spacer(minLength: 0)
                Button {
                    editing = MailComposeRequest(
                        draft: d,
                        onSent: { markSent() },
                        onClose: { edited in draft = edited }
                    )
                } label: {
                    Text("Edit").frame(minHeight: 36).padding(.horizontal, 6)
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .tint(.secondary)
                .disabled(sending)
                Button {
                    Task { await send(d) }
                } label: {
                    HStack(spacing: 6) {
                        if sending { ProgressView().tint(Color.limeInk) } else { Image(systemName: "paperplane.fill") }
                        Text("Send")
                    }
                    .font(.body.weight(.semibold))
                    .foregroundStyle(Color.limeInk)
                    .padding(.horizontal, 16)
                    .frame(minHeight: 36)
                    .background(Color.lime, in: .capsule)
                }
                .buttonStyle(.plain)
                .disabled(sending || d.to.isEmpty)
                .opacity(d.to.isEmpty ? 0.5 : 1)
                .accessibilityIdentifier("draft-send")
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(Color.subtle.opacity(0.6))
        }
        .background(.background, in: .rect(cornerRadius: 20))
        .clipShape(.rect(cornerRadius: 20))
        .shadow(color: .black.opacity(0.08), radius: 12, y: 4)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mail-draft-card")
    }

    private func field(_ label: String, _ value: String, weight: Font.Weight = .regular) -> some View {
        VStack(spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(label).font(.subheadline).foregroundStyle(.secondary).frame(width: 58, alignment: .leading)
                Text(verbatim: value).font(.callout.weight(weight)).frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 9)
            Divider().padding(.leading, 16)
        }
    }

    private func doneLine(icon: String, tone: Color, soft: Color, text: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .font(.system(size: 10, weight: .bold))
                .foregroundStyle(tone)
                .frame(width: 20, height: 20)
                .background(soft, in: .circle)
            Text(text).lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(.callout)
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .background(.background, in: .rect(cornerRadius: 14))
        .shadow(color: .black.opacity(0.05), radius: 6, y: 2)
    }

    private func markSent() {
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { sent = true }
        MailDrafts.mark(action.id, MailDrafts.sentKey)
    }

    private func send(_ d: MailDraft) async {
        guard let api = model.api else { return }
        if let bad = (d.to + d.cc).first(where: { !MailText.looksLikeAddress($0) }) {
            error = "“\(bad)” doesn't look like an email address. Tap Edit to fix it."
            return
        }
        sending = true
        defer { sending = false }
        do {
            try await api.sendMail(d)
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            markSent()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
