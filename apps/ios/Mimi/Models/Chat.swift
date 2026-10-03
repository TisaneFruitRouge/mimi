import Foundation

// Mirrors of `crates/protocol` types (chat.rs, providers.rs, people.rs). Only what the
// app reads is declared; unknown fields are ignored, so an older app keeps working
// against a newer computer.

nonisolated enum Locality: String, Codable, Sendable {
    case device, network, cloud
}

nonisolated struct ModelRef: Codable, Sendable, Hashable {
    var providerId: UUID
    var model: String
    enum CodingKeys: String, CodingKey { case providerId = "provider_id", model }
}

nonisolated struct Conversation: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var title: String
    var createdAt: Int64
    var updatedAt: Int64
    enum CodingKeys: String, CodingKey { case id, title, createdAt = "created_at", updatedAt = "updated_at" }
}

nonisolated enum MessageRole: String, Codable, Sendable { case user, assistant }

nonisolated enum MessageStatus: String, Codable, Sendable {
    case complete, streaming, error, cancelled, interrupted
}

nonisolated enum ActionStatus: String, Codable, Sendable {
    case pendingApproval = "pending_approval"
    case approved, rejected, running, done, failed
}

nonisolated struct Action: Codable, Sendable, Identifiable, Equatable {
    var id: UUID
    var tool: String
    var summary: String
    var arguments: JSONValue
    var requiresApproval: Bool
    var status: ActionStatus
    var result: String?
    var error: String?
    var output: JSONValue?
    var contentOffset: Int
    var alwaysAllow: String?

    enum CodingKeys: String, CodingKey {
        case id, tool, summary, arguments, status, result, error, output
        case requiresApproval = "requires_approval"
        case contentOffset = "content_offset"
        case alwaysAllow = "always_allow"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        tool = try c.decode(String.self, forKey: .tool)
        summary = try c.decode(String.self, forKey: .summary)
        arguments = try c.decodeIfPresent(JSONValue.self, forKey: .arguments) ?? .null
        requiresApproval = try c.decode(Bool.self, forKey: .requiresApproval)
        status = try c.decode(ActionStatus.self, forKey: .status)
        result = try c.decodeIfPresent(String.self, forKey: .result)
        error = try c.decodeIfPresent(String.self, forKey: .error)
        output = try c.decodeIfPresent(JSONValue.self, forKey: .output)
        contentOffset = try c.decodeIfPresent(Int.self, forKey: .contentOffset) ?? 0
        alwaysAllow = try c.decodeIfPresent(String.self, forKey: .alwaysAllow)
    }
}

nonisolated enum MentionKind: String, Codable, Sendable {
    case person, event
    case mailThread = "mail_thread"
    case mailMessage = "mail_message"

    var sigil: Character { self == .mailThread || self == .mailMessage ? "#" : "@" }
}

nonisolated struct Mention: Codable, Sendable, Hashable {
    var kind: MentionKind
    var id: String
    var label: String
}

nonisolated struct MentionCandidate: Codable, Sendable, Identifiable, Hashable {
    var kind: MentionKind
    var id: String
    var label: String
    var detail: String?
    var startsAt: Int64?
    enum CodingKeys: String, CodingKey { case kind, id, label, detail, startsAt = "starts_at" }
}

nonisolated struct Message: Codable, Sendable, Identifiable, Equatable {
    var id: UUID
    var conversationId: UUID
    var role: MessageRole
    var content: String
    var reasoning: String
    var status: MessageStatus
    var locality: Locality?
    var error: String?
    var createdAt: Int64
    var actions: [Action]
    var mentions: [Mention]

    enum CodingKeys: String, CodingKey {
        case id, role, content, reasoning, status, locality, error, actions, mentions
        case conversationId = "conversation_id"
        case createdAt = "created_at"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        conversationId = try c.decode(UUID.self, forKey: .conversationId)
        role = try c.decode(MessageRole.self, forKey: .role)
        content = try c.decode(String.self, forKey: .content)
        reasoning = try c.decodeIfPresent(String.self, forKey: .reasoning) ?? ""
        status = try c.decode(MessageStatus.self, forKey: .status)
        locality = try c.decodeIfPresent(Locality.self, forKey: .locality)
        error = try c.decodeIfPresent(String.self, forKey: .error)
        createdAt = try c.decode(Int64.self, forKey: .createdAt)
        actions = try c.decodeIfPresent([Action].self, forKey: .actions) ?? []
        mentions = try c.decodeIfPresent([Mention].self, forKey: .mentions) ?? []
    }
}

nonisolated struct ConversationDetail: Codable, Sendable {
    var conversation: Conversation
    var messages: [Message]
}

nonisolated struct SendMessage: Encodable, Sendable {
    var content: String
    var mentions: [Mention]
}

nonisolated struct SendMessageResult: Decodable, Sendable {
    var userMessage: Message
    var assistantMessage: Message
    enum CodingKeys: String, CodingKey { case userMessage = "user_message", assistantMessage = "assistant_message" }
}

nonisolated struct Provider: Codable, Sendable, Identifiable {
    var id: UUID
    var name: String
    var locality: Locality
}

/// The parts of `Settings` the app reads. Changes go through `AppModel.updateSettings`,
/// which edits the full JSON so fields this app doesn't know about are kept.
nonisolated struct Settings: Decodable, Sendable, Equatable {
    var assistantName: String
    var defaultModel: ModelRef?
    var onboardingDone: Bool

    enum CodingKeys: String, CodingKey {
        case assistantName = "assistant_name"
        case defaultModel = "default_model"
        case onboardingDone = "onboarding_done"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        assistantName = (try c.decodeIfPresent(String.self, forKey: .assistantName)).flatMap { $0.isEmpty ? nil : $0 } ?? "Mimi"
        defaultModel = try c.decodeIfPresent(ModelRef.self, forKey: .defaultModel)
        onboardingDone = try c.decodeIfPresent(Bool.self, forKey: .onboardingDone) ?? false
    }

    init(assistantName: String = "Mimi") {
        self.assistantName = assistantName
        defaultModel = nil
        onboardingDone = true
    }
}

nonisolated struct ApiErrorBody: Decodable, Sendable {
    var code: String
    var message: String
}
