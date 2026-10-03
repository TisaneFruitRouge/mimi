import SwiftUI

/// The tinted rounded-square icon of a settings row, like System Settings.
struct SettingsIcon: View {
    let systemImage: String
    var tint: Color
    var foreground: Color = .white
    var size: CGFloat = 29

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: size * 0.5, weight: .semibold))
            .foregroundStyle(foreground)
            .frame(width: size, height: size)
            .background(tint, in: .rect(cornerRadius: size * 0.24, style: .continuous))
            .accessibilityHidden(true)
    }
}

/// One row of the Settings list that opens a page: icon, title, and an optional quiet
/// value on the right. `SettingsLink("Privacy", "lock.fill", .privateTone) { PrivacySettings() }`.
struct SettingsLink<Destination: View>: View {
    let title: String
    let systemImage: String
    let tint: Color
    var foreground: Color = .white
    var value: String?
    @ViewBuilder let destination: () -> Destination

    init(_ title: String, _ systemImage: String, _ tint: Color, foreground: Color = .white, value: String? = nil,
         @ViewBuilder destination: @escaping () -> Destination) {
        self.title = title
        self.systemImage = systemImage
        self.tint = tint
        self.foreground = foreground
        self.value = value
        self.destination = destination
    }

    var body: some View {
        NavigationLink {
            destination()
        } label: {
            HStack(spacing: 12) {
                SettingsIcon(systemImage: systemImage, tint: tint, foreground: foreground)
                Text(title)
                Spacer(minLength: 8)
                if let value {
                    Text(value).foregroundStyle(.secondary).lineLimit(1)
                }
            }
        }
    }
}

/// A row with an icon, a title and a wrapping explanation under it.
struct ExplainedRow<Trailing: View>: View {
    let systemImage: String
    var tint: Color
    var foreground: Color = .white
    let title: String
    var detail: String?
    var detailColor: Color = .secondary
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            SettingsIcon(systemImage: systemImage, tint: tint, foreground: foreground)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                if let detail, !detail.isEmpty {
                    Text(detail)
                        .font(.footnote)
                        .foregroundStyle(detailColor)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
        }
        .padding(.vertical, 2)
    }
}

extension ExplainedRow where Trailing == EmptyView {
    init(systemImage: String, tint: Color, foreground: Color = .white, title: String, detail: String?, detailColor: Color = .secondary) {
        self.init(systemImage: systemImage, tint: tint, foreground: foreground, title: title, detail: detail,
                  detailColor: detailColor, trailing: { EmptyView() })
    }
}

/// Something that just happened, with a way to take it back: a capsule over the bottom
/// of the screen for a few seconds (the desktop's toast with Undo).
struct UndoNotice: Identifiable, Equatable {
    let id = UUID()
    var text: String
    var undoLabel = "Undo"
    var undo: (@MainActor () async throws -> Void)?

    static func == (a: UndoNotice, b: UndoNotice) -> Bool { a.id == b.id }
}

private struct UndoNoticeOverlay: ViewModifier {
    @Binding var notice: UndoNotice?
    @State private var error: String?

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .bottom) {
                if let notice {
                    HStack(spacing: 12) {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(Color.limeDeep)
                        Text(notice.text)
                            .font(.subheadline.weight(.medium))
                            .lineLimit(2)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        if let undo = notice.undo {
                            Button(notice.undoLabel) {
                                self.notice = nil
                                Task {
                                    do { try await undo() } catch { self.error = error.localizedDescription }
                                }
                            }
                            .font(.subheadline.weight(.semibold))
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .glassEffect(.regular, in: .capsule)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 10)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                    .task(id: notice.id) {
                        try? await Task.sleep(for: .seconds(8))
                        if self.notice?.id == notice.id { self.notice = nil }
                    }
                }
            }
            .animation(.spring(response: 0.4, dampingFraction: 0.85), value: notice)
            .problemAlert($error)
    }
}

extension View {
    /// Shows `notice` over the bottom of the view, with its Undo, for a few seconds.
    func undoNotice(_ notice: Binding<UndoNotice?>) -> some View {
        modifier(UndoNoticeOverlay(notice: notice))
    }

    /// The usual "Something went wrong" alert for a message meant for the user.
    func problemAlert(_ message: Binding<String?>, title: String = "Something went wrong") -> some View {
        alert(title, isPresented: Binding(get: { message.wrappedValue != nil }, set: { if !$0 { message.wrappedValue = nil } })) {
            Button("OK") { message.wrappedValue = nil }
        } message: {
            Text(message.wrappedValue ?? "")
        }
    }
}

extension Color {
    /// `#rrggbb` from the computer (a permission's or a calendar's colour).
    init(hexString: String?, fallback: Color = .gray) {
        guard let s = hexString?.trimmingCharacters(in: CharacterSet(charactersIn: "# ")), s.count == 6,
              let v = UInt32(s, radix: 16) else {
            self = fallback
            return
        }
        self.init(hex: v)
    }
}

/// A plain, calm line saying something can only be done on the computer.
struct OnYourComputerNote: View {
    let text: String

    var body: some View {
        Label {
            Text(text).font(.footnote).foregroundStyle(.secondary)
        } icon: {
            Image(systemName: "desktopcomputer").foregroundStyle(.secondary)
        }
    }
}
