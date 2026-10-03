import SwiftUI

/// Calendar colours: each calendar's own (from the computer), drawn as a light tint with
/// a 3pt bar, and the red of "now" and today, as calendar apps do.
nonisolated struct RGB: Equatable, Sendable {
    var r: Double, g: Double, b: Double

    /// `#rrggbb` (or `rrggbb`); nil for anything else.
    init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6, let v = UInt32(s, radix: 16) else { return nil }
        r = Double((v >> 16) & 0xFF) / 255
        g = Double((v >> 8) & 0xFF) / 255
        b = Double(v & 0xFF) / 255
    }

    init(r: Double, g: Double, b: Double) {
        self.r = r
        self.g = g
        self.b = b
    }

    /// `amount` of this colour, the rest `other`.
    func mixed(_ amount: Double, with other: RGB) -> RGB {
        RGB(r: r * amount + other.r * (1 - amount), g: g * amount + other.g * (1 - amount), b: b * amount + other.b * (1 - amount))
    }

    static let white = RGB(r: 1, g: 1, b: 1)
    static let ink = RGB(r: 0x1D / 255, g: 0x1D / 255, b: 0x1F / 255)
    static let grey = RGB(r: 0x8E / 255, g: 0x8E / 255, b: 0x93 / 255)

    var color: Color { Color(.sRGB, red: r, green: g, blue: b) }
}

/// The colours an event is drawn in, from its calendar's colour: readable on white,
/// never garish.
struct EventTint {
    let bar: Color
    let fill: Color
    let text: Color

    init(hex: String?) {
        let base = hex.flatMap(RGB.init(hex:)) ?? .grey
        bar = base.color
        fill = base.mixed(0.16, with: .white).color
        text = base.mixed(0.55, with: .ink).color
    }
}

extension Color {
    /// Today and the "now" line.
    static let calendarRed = Color(hex: 0xFF3B30)
}
