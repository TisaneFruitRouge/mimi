import Foundation
import Observation
import UIKit

/// Where the Mail tab is: a view of the mail, a smart folder, or one conversation.
enum MailRoute: Hashable {
    case box(MailBox)
    case folder(Int64)
    case thread(Int64)
}

/// A message being written: a reply, a forward, a new one, or a draft from the chat.
struct MailComposeRequest: Identifiable {
    let id = UUID()
    var draft: MailDraft
    /// The conversation it answers, for "Draft it for me".
    var thread: Int64?
    /// Ask the assistant for a reply as soon as it opens.
    var autoDraft = false
    var onSent: (() -> Void)?
    /// The edits so far, when it's closed without sending (draft cards keep them).
    var onClose: ((MailDraft) -> Void)?
}

/// The Mail tab's state: what's shown (per viewer, kept in UserDefaults like the desktop's
/// localStorage), the overview, and the actions every screen shares.
@Observable
final class MailStore {
    private(set) var overview: MailOverview?
    private(set) var loadError: String?
    /// Mail accounts that can't be reached, with what's wrong.
    private(set) var problems: [MailConnectionState] = []
    /// Whether an account is still fetching its mail for the first time.
    private(set) var checking = false
    /// Conversations being deleted or archived: gone from lists at once, back if it fails.
    private(set) var removing: Set<Int64> = []
    var path: [MailRoute] = []
    var compose: MailComposeRequest?
    /// A short confirmation at the bottom ("Archived").
    private(set) var toast: String?
    var error: String?
    /// Bumped by changes made here, so lists refresh without waiting for the event.
    private(set) var localRevision = 0

    private var toastTask: Task<Void, Never>?
    private var openedOnce = false

    static let viewKey = "mimi.mail.view"
    static let folderKey = "mimi.mail.folder"
    static let scopeKey = "mimi.mail.scope"

    var scopeState: MailScope = {
        guard let data = UserDefaults.standard.data(forKey: MailStore.scopeKey),
              let s = try? JSONDecoder().decode(MailScope.self, from: data) else { return .all }
        return s
    }()

    /// The scope in use: a remembered account or address that's gone means all mail.
    var scope: MailScope {
        guard let o = overview else { return scopeState }
        return Self.scopeRows(o).contains { $0.scope == scopeState } ? scopeState : .all
    }

    func setScope(_ s: MailScope) {
        scopeState = s
        if let data = try? JSONEncoder().encode(s) { UserDefaults.standard.set(data, forKey: Self.scopeKey) }
        localRevision += 1
    }

    // MARK: Loading

    func load(_ api: MimiAPI?) async {
        guard let api else { return }
        do {
            let o = try await api.mailOverview(scope)
            overview = o
            loadError = nil
            if !openedOnce, !o.accounts.isEmpty {
                openedOnce = true
                openStartingView(o)
            }
        } catch is CancellationError {
        } catch {
            if overview == nil { loadError = error.localizedDescription }
        }
        if let states = try? await api.mailConnections() {
            problems = states.filter { $0.status == "error" }
            checking = states.contains { $0.detail.hasPrefix("Checking") }
        }
    }

    /// The first time: straight into the last view (or what needs an answer), with the
    /// mailboxes one step back, as Mail does.
    private func openStartingView(_ o: MailOverview) {
        guard path.isEmpty else { return }
        let defaults = UserDefaults.standard
        if let folder = defaults.object(forKey: Self.folderKey) as? Int64, o.folders.contains(where: { $0.id == folder }) {
            path = [.folder(folder)]
            return
        }
        if let raw = defaults.string(forKey: Self.viewKey), let box = MailBox(rawValue: raw), o.sorting || !box.sorted {
            path = [.box(box)]
            return
        }
        path = [.box(o.sorting && o.needsReply > 0 ? .needsReply : .inbox)]
    }

    /// Remembers the view or folder opened last.
    func remember(_ route: MailRoute) {
        let defaults = UserDefaults.standard
        switch route {
        case .box(let b):
            defaults.set(b.rawValue, forKey: Self.viewKey)
            defaults.removeObject(forKey: Self.folderKey)
        case .folder(let id):
            defaults.set(id, forKey: Self.folderKey)
        case .thread:
            break
        }
    }

    // MARK: "Received on"

    struct ScopeRow: Identifiable, Hashable {
        var id: String
        var label: String
        var scope: MailScope
        var unread: Int?
        var nested: Bool
        var account: Bool
    }

    /// Each account when there are several, and each address mail arrived at when an
    /// account has more than one. Empty when there's only one address in all.
    static func scopeRows(_ o: MailOverview) -> [ScopeRow] {
        var rows: [ScopeRow] = []
        let several = o.accounts.count > 1
        for a in o.accounts {
            if several {
                rows.append(ScopeRow(id: a.connectionId.uuidString, label: a.email, scope: MailScope(account: a.connectionId),
                                     unread: nil, nested: false, account: true))
            }
            if a.addresses.count > 1 {
                for x in a.addresses {
                    rows.append(ScopeRow(id: "\(a.connectionId):\(x.email)", label: x.email, scope: MailScope(address: x.email),
                                         unread: x.unread, nested: several, account: false))
                }
            }
        }
        return rows
    }

    var scopeLabel: String? {
        guard let o = overview else { return nil }
        return Self.scopeRows(o).first { $0.scope == scope }?.label
    }

    // MARK: Actions every screen shares

    func say(_ text: String) {
        toast = text
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(2.4))
            guard !Task.isCancelled else { return }
            self?.toast = nil
        }
    }

    func changed() { localRevision += 1 }

    func archive(_ id: Int64, api: MimiAPI?) async {
        guard let api else { return }
        removing.insert(id)
        do {
            try await api.archiveMail(id)
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            say("Archived")
            changed()
        } catch {
            removing.remove(id)
            self.error = error.localizedDescription
        }
    }

    /// Moves a conversation to the Trash of its account (after the user confirmed).
    func delete(_ id: Int64, api: MimiAPI?) async {
        guard let api else { return }
        removing.insert(id)
        do {
            try await api.deleteMail(id)
            say("Moved to the Trash")
            changed()
        } catch {
            removing.remove(id)
            self.error = error.localizedDescription
        }
    }

    func markRead(_ id: Int64, read: Bool, api: MimiAPI?) async {
        guard let api else { return }
        do {
            try await api.markMailRead(id, read: read)
            changed()
        } catch {
            self.error = error.localizedDescription
        }
    }

    func move(_ thread: Int64, folder: MailFolder, member: Bool, api: MimiAPI?) async {
        guard let api else { return }
        do {
            try await api.setMailThreadFolder(thread, folder: folder.id, member: member)
            say(member ? "Added to “\(folder.name)”" : "Removed from “\(folder.name)”")
            changed()
            await load(api)
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Opens a reply (or reply all, or forward) to a conversation, fetching it first.
    func start(_ kind: ReplyKind, to id: Int64, api: MimiAPI?) async {
        guard let api else { return }
        do {
            let d = try await api.mailThread(id)
            let mine = overview?.ownAddresses ?? []
            let draft: MailDraft? = switch kind {
            case .reply: MailText.reply(to: d)
            case .replyAll: MailText.replyAll(to: d, mine: mine) ?? MailText.reply(to: d)
            case .forward: MailText.forward(d)
            }
            if let draft { compose = MailComposeRequest(draft: draft, thread: kind == .forward ? nil : id) }
        } catch {
            self.error = error.localizedDescription
        }
    }

    enum ReplyKind { case reply, replyAll, forward }

    // MARK: Attachments on this phone

    /// Opened attachments are kept in a folder of their own, emptied once per launch.
    static func attachmentsFolder() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("mail-attachments", isDirectory: true)
    }

    private static var cleaned = false
    static func cleanAttachmentsOnce() {
        guard !cleaned else { return }
        cleaned = true
        try? FileManager.default.removeItem(at: attachmentsFolder())
    }
}
