import QuickLook
import SwiftUI
import UIKit

/// The emails already fetched for showing, so switching modes or coming back is instant.
@MainActor
enum MailContentCache {
    static var plain: [Int64: MailContent] = [:]
    static var withImages: [Int64: MailContent] = [:]
}

/// Senders whose pictures load without asking ("Always for this sender"), on this phone.
enum MailTrustedSenders {
    static let key = "mimi.mail.images"

    static func contains(_ email: String) -> Bool {
        (UserDefaults.standard.stringArray(forKey: key) ?? []).contains(email.lowercased())
    }

    static func add(_ email: String) {
        var list = UserDefaults.standard.stringArray(forKey: key) ?? []
        list.append(email.lowercased())
        UserDefaults.standard.set(Array(list.suffix(500)), forKey: key)
    }
}

/// A message's body in the chosen mode. Text is the plain text the assistant reads,
/// never linkified; Formatted is tidy text made by the computer; Original is the email as
/// sent, made safe, with pictures from other servers held back until asked for.
struct MailMessageBody: View {
    let message: MailMessage
    let mode: MailMode
    var onMailTo: (String) -> Void = { _ in }
    @Environment(AppModel.self) private var model
    @State private var content: MailContent?
    @State private var failed = false
    @State private var loadingImages = false
    @State private var imageError: String?

    var body: some View {
        Group {
            if mode == .text || failed {
                Folded(text: message.body) { text, quoted in plain(text, quoted: quoted) }
            } else if let c = content {
                if mode == .original, let html = c.html {
                    VStack(alignment: .leading, spacing: 10) {
                        if c.remoteImages > 0 && !c.imagesLoaded {
                            imagesHidden
                        }
                        MailWebView(html: html, onMailTo: onMailTo)
                    }
                } else {
                    Folded(text: c.formatted) { text, quoted in
                        MarkdownText(text: text.isEmpty ? " " : text)
                            .opacity(quoted ? 0.75 : 1)
                    }
                }
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach([0, 40, 100], id: \.self) { short in
                        RoundedRectangle(cornerRadius: 5).fill(Color.subtle)
                            .frame(height: 13)
                            .padding(.trailing, CGFloat(short))
                    }
                }
                .accessibilityLabel("Loading the email")
            }
        }
        .task(id: "\(message.id)-\(mode != .text)") { await load() }
        .alert("Couldn't load the images", isPresented: .constant(imageError != nil)) {
            Button("OK") { imageError = nil }
        } message: {
            Text(imageError ?? "")
        }
    }

    private func plain(_ text: String, quoted: Bool) -> some View {
        // Plain text only: nothing in an email is turned into a link or formatting.
        Text(verbatim: text.isEmpty ? " " : text)
            .font(quoted ? .subheadline : .body)
            .foregroundStyle(quoted ? Color.secondary : Color.primary)
            .lineSpacing(2)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var imagesHidden: some View {
        let trusted = MailTrustedSenders.contains(message.from.email)
        return VStack(alignment: .leading, spacing: 8) {
            Label("Images are hidden to protect your privacy.", systemImage: "photo.badge.exclamationmark")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            HStack(spacing: 8) {
                Button {
                    Task { await loadImages() }
                } label: {
                    HStack(spacing: 6) {
                        if loadingImages { ProgressView().controlSize(.small) }
                        Text("Load images")
                    }
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .controlSize(.small)
                .disabled(loadingImages)
                // Never offered for suspicious mail, whoever it claims to be from.
                if !message.suspicious && !trusted {
                    Button("Always for this sender") {
                        MailTrustedSenders.add(message.from.email)
                        Task { await loadImages() }
                    }
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .disabled(loadingImages)
                }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.subtle, in: .rect(cornerRadius: 12))
    }

    private func load() async {
        guard mode != .text, let api = model.api else { return }
        if let cached = MailContentCache.withImages[message.id] ?? MailContentCache.plain[message.id] {
            content = cached
        } else {
            do {
                let c = try await api.mailContent(message.id)
                MailContentCache.plain[message.id] = c
                content = c
            } catch is CancellationError {
                return
            } catch {
                failed = true
                return
            }
        }
        // Pictures load by themselves only for senders the user trusts, and never in
        // suspicious mail.
        if let c = content, mode == .original, c.remoteImages > 0, !c.imagesLoaded,
           !message.suspicious, MailTrustedSenders.contains(message.from.email) {
            await loadImages()
        }
    }

    private func loadImages() async {
        guard let api = model.api else { return }
        loadingImages = true
        defer { loadingImages = false }
        do {
            let c = try await api.mailContentWithImages(message.id)
            MailContentCache.withImages[message.id] = c
            content = c
        } catch is CancellationError {
        } catch {
            imageError = error.localizedDescription
        }
    }
}

/// A body with its quoted history behind a ••• button.
private struct Folded<Body: View>: View {
    let text: String
    @ViewBuilder let render: (String, Bool) -> Body
    @State private var showQuoted = false

    var body: some View {
        let (main, quoted) = MailText.splitQuoted(text)
        VStack(alignment: .leading, spacing: 10) {
            render(main, false)
            if let quoted {
                Button {
                    withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { showQuoted.toggle() }
                } label: {
                    Text("•••")
                        .font(.footnote.weight(.bold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 2)
                        .background(Color(hex: 0x767680, opacity: 0.12), in: .capsule)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(showQuoted ? "Hide quoted text" : "Show quoted text")
                if showQuoted {
                    render(quoted, true)
                        .padding(.leading, 12)
                        .overlay(alignment: .leading) {
                            Capsule().fill(Color.secondary.opacity(0.25)).frame(width: 2)
                        }
                }
            }
        }
    }
}

// MARK: - Attachments

/// A message's attachments: fetched from the mail server through the computer when
/// tapped, then shown with Quick Look. Programs and scripts are only saved to Files.
struct MailAttachments: View {
    let message: MailMessage
    @Environment(AppModel.self) private var model
    @State private var opening: Int?
    @State private var preview: URL?
    @State private var program: (name: String, url: URL)?
    @State private var exporting: ExportItem?
    @State private var error: String?

    var body: some View {
        FlowLayout(spacing: 6, lineSpacing: 6) {
            ForEach(Array(message.attachments.enumerated()), id: \.offset) { i, name in
                Button {
                    Task { await open(i, name: name) }
                } label: {
                    HStack(spacing: 5) {
                        if opening == i {
                            ProgressView().controlSize(.mini)
                        } else {
                            Image(systemName: MailText.isProgram(name) ? "exclamationmark.triangle" : "paperclip")
                                .font(.caption.weight(.semibold))
                        }
                        Text(name).lineLimit(1).truncationMode(.middle)
                    }
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .frame(maxWidth: 260)
                    .background(Color(hex: 0x767680, opacity: 0.12), in: .capsule)
                }
                .buttonStyle(.plain)
                .disabled(opening != nil)
                .accessibilityLabel("Open \(name)")
            }
        }
        .quickLookPreview($preview)
        .alert(
            "Save “\(program?.name ?? "")” to Files?",
            isPresented: .constant(program != nil)
        ) {
            Button("Save to Files") {
                exporting = program.map { ExportItem(url: $0.url) }
                program = nil
            }
            Button("Cancel", role: .cancel) { program = nil }
        } message: {
            Text("It's a program or script, so \(model.assistantName) doesn't open it for you. Only run it if you trust who sent it.")
        }
        .sheet(item: $exporting) { item in
            ExportToFiles(url: item.url).ignoresSafeArea()
        }
        .alert("Couldn't open the attachment", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    private func open(_ index: Int, name: String) async {
        guard let api = model.api else { return }
        opening = index
        defer { opening = nil }
        do {
            let data = try await api.mailAttachment(message: message.id, index: index)
            let folder = MailStore.attachmentsFolder().appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let url = folder.appendingPathComponent(MailText.safeFileName(name))
            try data.write(to: url, options: [.atomic, .completeFileProtection])
            if MailText.isProgram(name) {
                program = (name, url)
            } else {
                preview = url
            }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// A file to save, as a sheet item.
private struct ExportItem: Identifiable {
    let url: URL
    var id: URL { url }
}

/// The system's "Save to Files" for one file, a copy.
private struct ExportToFiles: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        UIDocumentPickerViewController(forExporting: [url], asCopy: true)
    }

    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
}
