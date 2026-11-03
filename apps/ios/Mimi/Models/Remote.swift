import Foundation

// Mirrors of `crates/protocol/src/remote.rs` and events.rs.

nonisolated enum DevicePath: String, Codable, Sendable { case direct, relayed }

nonisolated struct Device: Codable, Sendable, Identifiable, Equatable {
    var id: UUID
    var name: String
    var createdAt: Int64
    var lastSeenAt: Int64?
    var connection: DevicePath?
    var thisDevice: Bool

    enum CodingKeys: String, CodingKey {
        case id, name, connection
        case createdAt = "created_at"
        case lastSeenAt = "last_seen_at"
        case thisDevice = "this_device"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        createdAt = try c.decode(Int64.self, forKey: .createdAt)
        lastSeenAt = try c.decodeIfPresent(Int64.self, forKey: .lastSeenAt)
        connection = try c.decodeIfPresent(DevicePath.self, forKey: .connection)
        thisDevice = try c.decodeIfPresent(Bool.self, forKey: .thisDevice) ?? false
    }
}

nonisolated struct Relay: Codable, Sendable, Equatable {
    var kind: String
    var url: String?
}

nonisolated struct RemoteAccess: Codable, Sendable, Equatable {
    var running: Bool
    var relay: Relay
    var relayConnected: Bool
    var devices: [Device]

    enum CodingKeys: String, CodingKey {
        case running, relay, devices
        case relayConnected = "relay_connected"
    }
}

nonisolated struct PairRequest: Encodable, Sendable {
    var code: String
    var name: String
}

nonisolated struct Paired: Decodable, Sendable {
    var device: Device
    var token: String
}

nonisolated struct Health: Decodable, Sendable {
    var version: String
}

/// Something that happened on the computer, pushed over the event feed. Only the events
/// the app acts on are decoded in detail; the rest just say "something changed".
nonisolated enum Event: Decodable, Sendable {
    case settingsChanged(JSONValue)
    case providersChanged([Provider])
    case conversationUpdated(Conversation)
    case conversationDeleted(UUID)
    case messageUpdated(Message)
    case messageDelta(conversationId: UUID, messageId: UUID, content: String, reasoning: String)
    case remoteChanged(RemoteAccess)
    case scheduleDelivered(JSONValue)
    case resync
    /// Mail, people, calendars, memory, reminders…: refetch what's on screen.
    case changed(String)

    private enum Keys: String, CodingKey {
        case type, settings, providers, conversation, id, message, remote, delivery
        case conversationId = "conversation_id"
        case messageId = "message_id"
        case content, reasoning
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "settings_changed": self = .settingsChanged(try c.decode(JSONValue.self, forKey: .settings))
        case "providers_changed": self = .providersChanged(try c.decode([Provider].self, forKey: .providers))
        case "conversation_updated": self = .conversationUpdated(try c.decode(Conversation.self, forKey: .conversation))
        case "conversation_deleted": self = .conversationDeleted(try c.decode(UUID.self, forKey: .id))
        case "message_updated": self = .messageUpdated(try c.decode(Message.self, forKey: .message))
        case "message_delta":
            self = .messageDelta(
                conversationId: try c.decode(UUID.self, forKey: .conversationId),
                messageId: try c.decode(UUID.self, forKey: .messageId),
                content: try c.decode(String.self, forKey: .content),
                reasoning: try c.decodeIfPresent(String.self, forKey: .reasoning) ?? ""
            )
        case "remote_changed": self = .remoteChanged(try c.decode(RemoteAccess.self, forKey: .remote))
        case "schedule_delivered": self = .scheduleDelivered(try c.decode(JSONValue.self, forKey: .delivery))
        case "resync": self = .resync
        default: self = .changed(type)
        }
    }
}
