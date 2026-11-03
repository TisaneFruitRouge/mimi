import SwiftUI

/// How each way to reach someone is named and drawn (the desktop's `channelInfo`).
extension Channel {
    var label: String {
        switch self {
        case .phone: "Phone"
        case .email: "Email"
        case .telegram: "Telegram"
        case .signal: "Signal"
        case .whatsapp: "WhatsApp"
        case .matrix: "Matrix"
        case .other: "Other"
        }
    }

    var symbol: String {
        switch self {
        case .phone: "phone.fill"
        case .email: "envelope.fill"
        case .telegram: "paperplane.fill"
        case .signal: "lock.shield.fill"
        case .whatsapp: "message.fill"
        case .matrix: "number"
        case .other: "at"
        }
    }

    var tint: Color {
        switch self {
        case .phone: Color(hex: 0x2F7A4F)
        case .email: Color(hex: 0x6146AD)
        case .telegram: Color(hex: 0x136C86)
        case .signal: Color(hex: 0x3353A8)
        case .whatsapp: Color(hex: 0x1D7A3A)
        case .matrix: .ink
        case .other: .secondary
        }
    }

    var placeholder: String {
        switch self {
        case .phone, .signal, .whatsapp: "+41 79 123 45 67"
        case .email: "name@example.com"
        case .telegram: "@username"
        case .matrix: "@name:matrix.org"
        case .other: ""
        }
    }

    /// Labels offered for each way to reach someone (any other can be typed).
    var labelChoices: [String] {
        switch self {
        case .phone: ["mobile", "home", "work"]
        case .email: ["home", "work"]
        case .signal, .whatsapp: ["mobile", "work"]
        default: []
        }
    }

    /// The ones a person can be given by hand.
    static let addable: [Channel] = [.phone, .email, .telegram, .signal, .whatsapp, .matrix]
}

struct ChannelGlyph: View {
    let channel: Channel
    var size: CGFloat = 13

    var body: some View {
        Image(systemName: channel.symbol)
            .font(.system(size: size, weight: .medium))
            .foregroundStyle(channel.tint)
            .accessibilityLabel(channel.label)
    }
}

/// A row of small channel icons, as in the list and pickers.
struct ChannelGlyphs: View {
    let channels: [Channel]

    var body: some View {
        HStack(spacing: 5) {
            ForEach(channels, id: \.self) { ChannelGlyph(channel: $0, size: 11) }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(channels.map(\.label).joined(separator: ", "))
    }
}

/// Initials in a colour that stays the same for the same person, here and on the
/// computer (the same hash as the desktop's `PersonAvatar`).
struct PersonAvatar: View {
    let id: UUID
    let name: String
    var size: CGFloat = 36

    nonisolated private static let tones: [(bg: UInt32, fg: UInt32)] = [
        (0xE5EEFC, 0x1F64C7),
        (0xE1F4EC, 0x0B7A5C),
        (0xFDF0DC, 0x9A5600),
        (0xEFE9FB, 0x6146AD),
        (0xFBE8EC, 0xA83250),
        (0xDEF3F7, 0x136C86),
    ]

    static func tone(for id: UUID) -> (bg: Color, fg: Color) {
        let t = tones[toneIndex(id.uuidString.lowercased())]
        return (Color(hex: t.bg), Color(hex: t.fg))
    }

    /// `h = (h * 31 + c) | 0` over the id's characters, as JavaScript does it.
    nonisolated static func toneIndex(_ s: String) -> Int {
        var h: Int32 = 0
        for scalar in s.unicodeScalars {
            h = h &* 31 &+ Int32(truncatingIfNeeded: scalar.value)
        }
        return Int(abs(Int64(h)) % Int64(tones.count))
    }

    var body: some View {
        let tone = Self.tone(for: id)
        Text(personInitials(name))
            .font(.system(size: size * 0.38, weight: .semibold))
            .foregroundStyle(tone.fg)
            .frame(width: size, height: size)
            .background(tone.bg, in: .circle)
            .accessibilityHidden(true)
    }
}

/// Overlapping avatars, for several people at once.
struct AvatarStack: View {
    let people: [(id: UUID, name: String)]
    var size: CGFloat = 32

    var body: some View {
        HStack(spacing: -size * 0.3) {
            ForEach(Array(people.prefix(5).enumerated()), id: \.offset) { _, p in
                PersonAvatar(id: p.id, name: p.name, size: size)
                    .overlay(Circle().stroke(Color(.systemBackground), lineWidth: 2))
            }
        }
    }
}

/// A label as people read it: "mobile" → "Mobile"; address books' own words kept.
func handleLabelText(_ label: String) -> String { label.upperFirst }

/// The quiet line under a handle: "Mobile · Phone · from iCloud".
func handleDetail(_ h: Handle) -> String {
    var parts: [String] = []
    if let label = h.label, !label.isEmpty {
        parts.append(handleLabelText(label))
        parts.append(h.channel.label)
    } else {
        parts.append(h.channel.label)
    }
    parts.append(h.isOwn ? "added by you" : "from \(h.sourceName)")
    return parts.joined(separator: " · ")
}
