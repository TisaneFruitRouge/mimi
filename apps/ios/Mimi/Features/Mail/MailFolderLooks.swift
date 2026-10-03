import SwiftUI

/// How smart folders can look. The names match the computer's `mail::folders::ICONS` and
/// `COLORS` (it refuses anything else) and the desktop's `folder-looks.tsx`; unknown names
/// fall back to the first of each.
enum MailFolderLooks {
    /// Icon names in the computer's order, with the SF Symbol drawn for each.
    static let icons: [(name: String, symbol: String)] = [
        ("sparkles", "sparkles"),
        ("folder", "folder"),
        ("receipt", "receipt"),
        ("plane", "airplane"),
        ("briefcase", "briefcase"),
        ("heart", "heart"),
        ("house", "house"),
        ("shopping-bag", "bag"),
        ("graduation-cap", "graduationcap"),
        ("baby", "stroller"),
        ("paw-print", "pawprint"),
        ("car", "car"),
        ("stethoscope", "stethoscope"),
        ("landmark", "building.columns"),
        ("newspaper", "newspaper"),
        ("users", "person.2"),
        ("star", "star"),
        ("gift", "gift"),
        ("utensils", "fork.knife"),
        ("dumbbell", "dumbbell"),
        ("music", "music.note"),
        ("code", "chevron.left.forwardslash.chevron.right"),
        ("piggy-bank", "banknote"),
        ("calendar", "calendar"),
    ]

    /// Each colour as a soft fill and a darker ink readable on it and on white.
    static let colors: [(name: String, label: String, soft: UInt32, ink: UInt32)] = [
        ("violet", "Violet", 0xEFE9FB, 0x6146AD),
        ("blue", "Blue", 0xE6EFFD, 0x1F64C7),
        ("teal", "Teal", 0xE3F5EE, 0x0A7A5B),
        ("green", "Green", 0xEAF6E1, 0x3F7A1E),
        ("yellow", "Yellow", 0xFCF3D4, 0x8A6A00),
        ("orange", "Orange", 0xFDEEDC, 0xB25A00),
        ("red", "Red", 0xFDEAEA, 0xC2311F),
        ("pink", "Pink", 0xFCE9F2, 0xB0306E),
        ("gray", "Gray", 0xEFEFF2, 0x5B5B62),
    ]

    static func symbol(_ icon: String) -> String {
        icons.first { $0.name == icon }?.symbol ?? icons[0].symbol
    }

    static func ink(_ color: String) -> Color {
        Color(hex: (colors.first { $0.name == color } ?? colors[0]).ink)
    }

    static func soft(_ color: String) -> Color {
        Color(hex: (colors.first { $0.name == color } ?? colors[0]).soft)
    }
}

/// A folder's icon in its colour.
struct MailFolderGlyph: View {
    let icon: String
    let color: String

    var body: some View {
        Image(systemName: MailFolderLooks.symbol(icon))
            .foregroundStyle(MailFolderLooks.ink(color))
    }
}

/// A folder as a small tinted tile, for lists.
struct MailFolderTile: View {
    let icon: String
    let color: String
    var size: CGFloat = 30

    var body: some View {
        Image(systemName: MailFolderLooks.symbol(icon))
            .font(.system(size: size * 0.5, weight: .medium))
            .foregroundStyle(MailFolderLooks.ink(color))
            .frame(width: size, height: size)
            .background(MailFolderLooks.soft(color), in: .rect(cornerRadius: size * 0.3))
    }
}
