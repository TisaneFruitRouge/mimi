import SwiftUI

/// Mimi's colours, the same tokens as the desktop (`apps/desktop/src/index.css`). Lime is
/// the signature, used sparingly: send, Approve, success. Data-flow colours have fixed
/// meanings: private (teal), network (blue), cloud (amber).
extension Color {
    static let lime = Color(hex: 0xC8F25D)
    static let limeSoft = Color(hex: 0xF0F9D9)
    static let limeDeep = Color(hex: 0x56760D)
    static let limeInk = Color(hex: 0x1D1D1F)
    static let canvas = Color(hex: 0xF5F5F7)
    static let ink = Color(hex: 0x1D1D1F)
    static let subtle = Color(hex: 0xF0F0F3)
    static let userBubble = Color(hex: 0xE9E9EE)
    static let privateTone = Color(hex: 0x0A7A5B)
    static let privateSoft = Color(hex: 0xE3F5EE)
    static let networkTone = Color(hex: 0x1F64C7)
    static let networkSoft = Color(hex: 0xE6EFFD)
    static let cloudTone = Color(hex: 0x975400)
    static let cloudSoft = Color(hex: 0xFDF1DF)
    static let danger = Color(hex: 0xD12F1C)
    static let dangerSoft = Color(hex: 0xFDEEEC)

    init(hex: UInt32, opacity: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
        )
    }
}

/// The lime button: the one signature positive action on a screen (send, Approve, Pair).
struct LimeButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.semibold))
            .foregroundStyle(Color.limeInk)
            .padding(.horizontal, 20)
            .frame(minHeight: 50)
            .frame(maxWidth: .infinity)
            .background(Color.lime, in: .capsule)
            .opacity(isEnabled ? 1 : 0.5)
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.7), value: configuration.isPressed)
    }
}

extension ButtonStyle where Self == LimeButtonStyle {
    static var lime: LimeButtonStyle { LimeButtonStyle() }
}

/// "On this computer", "On your network", "Cloud": where a model runs. Cloud must always
/// be visible.
struct LocalityBadge: View {
    let locality: Locality
    var compact = false

    var body: some View {
        Label {
            if !compact { Text(label) }
        } icon: {
            Image(systemName: icon)
        }
        .font(.caption.weight(.medium))
        .foregroundStyle(tone)
        .padding(.horizontal, compact ? 6 : 9)
        .padding(.vertical, 4)
        .background(soft, in: .capsule)
        .accessibilityLabel(label)
    }

    private var label: String {
        switch locality {
        case .device: "On your computer"
        case .network: "On your network"
        case .cloud: "Cloud"
        }
    }

    private var icon: String {
        switch locality {
        case .device: "checkmark.shield.fill"
        case .network: "house.fill"
        case .cloud: "cloud.fill"
        }
    }

    private var tone: Color {
        switch locality {
        case .device: .privateTone
        case .network: .networkTone
        case .cloud: .cloudTone
        }
    }

    private var soft: Color {
        switch locality {
        case .device: .privateSoft
        case .network: .networkSoft
        case .cloud: .cloudSoft
        }
    }
}

nonisolated extension String {
    var upperFirst: String { prefix(1).uppercased() + dropFirst() }
    var lowerFirst: String { prefix(1).lowercased() + dropFirst() }
}

/// "5 min ago", "Yesterday", "Mon", "12 Mar".
func relativeDay(_ ms: Int64) -> String {
    let date = Date(timeIntervalSince1970: Double(ms) / 1000)
    let cal = Calendar.current
    if cal.isDateInToday(date) { return date.formatted(date: .omitted, time: .shortened) }
    if cal.isDateInYesterday(date) { return "Yesterday" }
    if let days = cal.dateComponents([.day], from: date, to: .now).day, days < 7 {
        return date.formatted(.dateTime.weekday(.wide))
    }
    return date.formatted(.dateTime.day().month(.abbreviated))
}
