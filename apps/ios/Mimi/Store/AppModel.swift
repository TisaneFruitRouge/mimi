import Foundation
import Observation
import UIKit

enum AppTab: Hashable { case chats, calendar, mail, people, settings }

/// Text for the composer, with the tags it contains.
struct Draft: Equatable {
    var text: String
    var mentions: [Mention]
}

/// Everything the app knows, kept in step with the computer by its event feed (like the
/// desktop's `lib/events.ts`): views read from here and call the API; events, not
/// refetches, bring most changes in.
@Observable
final class AppModel {
    enum Phase: Equatable {
        /// No computer yet: the welcome and pairing.
        case unpaired(reason: String?)
        case connecting
        case online
        /// Paired, but the computer can't be reached right now.
        case offline
    }

    private(set) var phase: Phase
    private(set) var paired: PairedComputer?
    private(set) var api: MimiAPI?

    private(set) var settings = Settings()
    /// Every setting as the computer has it, for pages that read fields `Settings` doesn't.
    private(set) var settingsJSON: JSONValue = .null
    private(set) var providers: [Provider] = []
    private(set) var conversations: [Conversation] = []
    private(set) var details: [UUID: ConversationDetail] = [:]
    private(set) var remote: RemoteAccess?
    /// How this phone reaches the computer right now.
    private(set) var path: LinkPath?
    /// Bumped when something of that kind changes on the computer (`mail_changed`,
    /// `people_changed`…), so screens showing it refetch: `.task(id: model.revision(…))`.
    private(set) var revisions: [String: Int] = [:]
    /// A reminder or routine result that just arrived, shown as a banner.
    var delivery: JSONValue?
    /// A pairing link opened from outside (the Camera app scanned the code).
    var incomingLink: PairingLink?
    /// The tab on screen.
    var tab: AppTab = .chats
    /// A message to start a new chat with ("Ask Mimi about this"), taken by the chat once.
    var askDraft: Draft?

    private var feed: Task<Void, Never>?
    private var retry: Task<Void, Never>?
    private var retryDelay: Double = 2

    init() {
        if let paired = PairedComputer.load() {
            self.paired = paired
            self.api = MimiAPI(link: DaemonLink(endpointId: paired.endpointId, relay: paired.relay, token: paired.token))
            phase = .connecting
        } else {
            phase = .unpaired(reason: nil)
        }
    }

    var assistantName: String { settings.assistantName }

    /// Where the model in use runs, for the privacy chip.
    var modelLocality: Locality? {
        guard let id = settings.defaultModel?.providerId else { return nil }
        return providers.first { $0.id == id }?.locality
    }

    func revision(_ kind: String) -> Int { revisions[kind, default: 0] }

    // MARK: Pairing

    /// Trades the code from the QR code for this phone's own token.
    func pair(with link: PairingLink, name: String) async throws {
        let daemon = DaemonLink(endpointId: link.endpointId, relay: link.relay, token: nil)
        let api = MimiAPI(link: daemon)
        let result = try await api.pair(code: link.code, name: name)
        let paired = PairedComputer(
            endpointId: link.endpointId,
            relay: link.relay,
            token: result.token,
            deviceId: result.device.id,
            deviceName: result.device.name
        )
        paired.save()
        await daemon.setToken(result.token)
        self.paired = paired
        self.api = api
        phase = .connecting
        await refresh()
    }

    /// Forgets the computer on this phone, and tells the computer if it can be reached.
    func unpair(tellComputer: Bool = true) async {
        if tellComputer, let api, let paired {
            try? await api.removeThisPhone(paired.deviceId)
        }
        forget(reason: nil)
    }

    private func forget(reason: String?) {
        feed?.cancel()
        retry?.cancel()
        PairedComputer.forget()
        // A removed phone starts over with a new identity.
        DaemonLink.forgetPhoneKey()
        paired = nil
        api = nil
        conversations = []
        details = [:]
        remote = nil
        phase = .unpaired(reason: reason)
    }

    // MARK: Connection

    /// Loads everything and listens for changes. Called at launch and when the app comes
    /// back to the foreground.
    func refresh() async {
        guard let api else { return }
        retry?.cancel()
        if phase != .online { phase = .connecting }
        do {
            async let settingsJSON = api.settingsJSON()
            async let providers = api.providers()
            async let conversations = api.conversations()
            let (s, p, c) = try await (settingsJSON, providers, conversations)
            applySettings(s)
            self.providers = p
            self.conversations = c.sorted { $0.updatedAt > $1.updatedAt }
            // Open conversations may have moved on while we were away.
            for id in details.keys { await loadConversation(id) }
            remote = try? await api.remote()
            path = await api.link.path()
            phase = .online
            retryDelay = 2
            bumpAll()
            listen()
        } catch {
            handle(error)
        }
    }

    /// The app went to the background: stop listening (iOS would cut it off anyway).
    func pause() {
        feed?.cancel()
        feed = nil
        retry?.cancel()
    }

    func resume() async {
        guard let api else { return }
        await api.link.reset()
        await refresh()
    }

    private func listen() {
        feed?.cancel()
        guard let api else { return }
        feed = Task { [weak self] in
            do {
                let events = try await api.events()
                for try await event in events {
                    guard let self, !Task.isCancelled else { return }
                    if let event { self.apply(event) }
                }
            } catch {
                guard let self, !Task.isCancelled else { return }
                self.handle(error)
                return
            }
            // The feed ended: the connection dropped.
            guard let self, !Task.isCancelled else { return }
            self.handle(MimiError.unreachable)
        }
    }

    /// What to do when a call fails: a removed phone starts over, anything else waits
    /// and tries again.
    func handle(_ error: Error) {
        if let error = error as? MimiError, error == .unpaired {
            forget(reason: "This phone was removed from your computer. Pair it again to keep using \(assistantName).")
            return
        }
        guard api != nil, !(error is CancellationError) else { return }
        phase = .offline
        path = nil
        retry?.cancel()
        let delay = retryDelay
        retryDelay = min(retryDelay * 2, 60)
        retry = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard let self, !Task.isCancelled else { return }
            await self.api?.link.reset()
            await self.refresh()
        }
    }

    func retryNow() {
        retryDelay = 2
        Task { await resume() }
    }

    // MARK: Events

    private func apply(_ event: Event) {
        switch event {
        case .settingsChanged(let json):
            applySettings(json)
        case .providersChanged(let list):
            providers = list
        case .conversationUpdated(let c):
            conversations.removeAll { $0.id == c.id }
            conversations.append(c)
            conversations.sort { $0.updatedAt > $1.updatedAt }
            details[c.id]?.conversation = c
        case .conversationDeleted(let id):
            conversations.removeAll { $0.id == id }
            details[id] = nil
        case .messageUpdated(let m):
            upsert(m)
        case .messageDelta(let conversationId, let messageId, let content, let reasoning):
            guard var detail = details[conversationId],
                  let i = detail.messages.firstIndex(where: { $0.id == messageId }) else { return }
            detail.messages[i].content += content
            detail.messages[i].reasoning += reasoning
            details[conversationId] = detail
        case .remoteChanged(let r):
            // The computer's own view says nothing about which phone is asking.
            var r = r
            if let me = paired?.deviceId {
                for i in r.devices.indices { r.devices[i].thisDevice = r.devices[i].id == me }
            }
            remote = r
            Task { path = await api?.link.path() }
        case .scheduleDelivered(let delivery):
            self.delivery = delivery
            revisions["schedule_changed", default: 0] += 1
        case .resync:
            Task { await refresh() }
        case .changed(let kind):
            revisions[kind, default: 0] += 1
        }
    }

    private func bumpAll() {
        for kind in ["mail_changed", "people_changed", "connections_changed", "memory_changed", "schedule_changed"] {
            revisions[kind, default: 0] += 1
        }
    }

    private func applySettings(_ json: JSONValue) {
        settingsJSON = json
        if let data = try? JSONEncoder().encode(json), let s = try? JSONDecoder().decode(Settings.self, from: data) {
            settings = s
        }
    }

    private func upsert(_ m: Message) {
        guard var detail = details[m.conversationId] else { return }
        if let i = detail.messages.firstIndex(where: { $0.id == m.id }) {
            detail.messages[i] = m
        } else {
            detail.messages.append(m)
        }
        details[m.conversationId] = detail
    }

    // MARK: Settings

    /// Changes some settings, keeping every other field as the computer has it.
    func updateSettings(_ changes: [String: JSONValue]) async throws {
        guard let api, case .object(var object) = settingsJSON else { return }
        for (k, v) in changes { object[k] = v }
        applySettings(try await api.putSettings(.object(object)))
    }

    // MARK: Chat

    func loadConversation(_ id: UUID) async {
        guard let api else { return }
        do {
            details[id] = try await api.conversation(id)
        } catch {
            handle(error)
        }
    }

    func newConversation() async throws -> Conversation {
        guard let api else { throw MimiError.unreachable }
        let c = try await api.createConversation()
        if !conversations.contains(where: { $0.id == c.id }) { conversations.insert(c, at: 0) }
        details[c.id] = ConversationDetail(conversation: c, messages: [])
        return c
    }

    func send(_ text: String, mentions: [Mention], in id: UUID) async throws {
        guard let api else { throw MimiError.unreachable }
        let result = try await api.send(id, SendMessage(content: text, mentions: mentions))
        upsert(result.userMessage)
        upsert(result.assistantMessage)
    }

    func rename(_ id: UUID, to title: String) async throws {
        guard let api else { return }
        let c = try await api.renameConversation(id, title: title)
        apply(.conversationUpdated(c))
    }

    func delete(_ id: UUID) async throws {
        guard let api else { return }
        try await api.deleteConversation(id)
        apply(.conversationDeleted(id))
    }

    func renameThisPhone(_ name: String) async throws {
        guard let api, var paired else { return }
        try await api.renamePhone(paired.deviceId, name: name)
        paired.deviceName = name
        paired.save()
        self.paired = paired
    }

    /// "Ask Mimi about this": a new chat with the thing already tagged, ready to finish
    /// typing. Every panel offers it (people, events, emails).
    func ask(about kind: MentionKind, id: String, label: String) {
        askDraft = Draft(text: "\(kind.sigil)\(label) ", mentions: [Mention(kind: kind, id: id, label: label)])
        tab = .chats
    }

    /// A default name for this phone on the computer's list.
    static var defaultPhoneName: String { UIDevice.current.model }
}
