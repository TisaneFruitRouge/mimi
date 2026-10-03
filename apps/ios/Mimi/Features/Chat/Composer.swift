import SwiftUI

/// The message field, floating over the thread: @ for people and events, # for email,
/// the privacy chip, and the round send/stop button.
struct Composer: View {
    let replying: Bool
    let onSend: (String, [Mention]) async throws -> Void
    let onStop: () async -> Void

    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var mentions: [Mention] = []
    @State private var suggestions: [MentionCandidate] = []
    @State private var sending = false
    @State private var error: String?
    @FocusState private var focused: Bool

    /// The @ or # word being typed at the end of the text, if any.
    private var query: (sigil: Character, text: String)? {
        guard let last = text.split(separator: " ", omittingEmptySubsequences: false).last,
              let first = last.first, first == "@" || first == "#" else { return nil }
        // Labels can have spaces ("Sam Carter"); the picker only searches the first word.
        return (first, String(last.dropFirst()))
    }

    var body: some View {
        VStack(spacing: 8) {
            if !suggestions.isEmpty {
                MentionSuggestions(candidates: suggestions) { pick($0) }
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
            VStack(alignment: .leading, spacing: 6) {
                TextField("Message \(model.assistantName)", text: $text, axis: .vertical)
                    .lineLimit(1...6)
                    .focused($focused)
                    .padding(.horizontal, 6)
                    .padding(.top, 6)
                HStack(spacing: 10) {
                    Button {
                        insert("@")
                    } label: {
                        Image(systemName: "at")
                            .frame(width: 32, height: 32)
                            .background(Color.subtle, in: .circle)
                    }
                    .accessibilityLabel("Mention someone or an event")
                    Button {
                        insert("#")
                    } label: {
                        Image(systemName: "number")
                            .frame(width: 32, height: 32)
                            .background(Color.subtle, in: .circle)
                    }
                    .accessibilityLabel("Mention an email")
                    Spacer()
                    if let locality = model.modelLocality {
                        LocalityBadge(locality: locality, compact: locality != .cloud)
                    }
                    sendButton
                }
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(.secondary)
            }
            .padding(10)
            .glassEffect(.regular, in: .rect(cornerRadius: 24))
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.85), value: suggestions.isEmpty)
        .task(id: query.map { "\($0.sigil)\($0.text)" }) { await suggest() }
        .alert("Couldn't send", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    @ViewBuilder private var sendButton: some View {
        if replying {
            Button {
                Task { await onStop() }
            } label: {
                Image(systemName: "stop.fill")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 34, height: 34)
                    .background(Color.ink, in: .circle)
            }
            .accessibilityLabel("Stop")
        } else {
            let empty = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            Button(action: send) {
                Group {
                    if sending { ProgressView().tint(.limeInk) } else { Image(systemName: "arrow.up") }
                }
                .font(.system(size: 16, weight: .bold))
                .foregroundStyle(Color.limeInk)
                .frame(width: 34, height: 34)
                .background(empty ? Color.subtle : Color.lime, in: .circle)
            }
            .disabled(empty || sending)
            .accessibilityLabel("Send")
        }
    }

    private func insert(_ sigil: String) {
        if !text.isEmpty, !text.hasSuffix(" ") { text += " " }
        text += sigil
        focused = true
    }

    private func pick(_ c: MentionCandidate) {
        guard let q = query else { return }
        text.removeLast(q.text.count + 1)
        text += "\(c.kind.sigil)\(c.label) "
        if !mentions.contains(where: { $0.kind == c.kind && $0.id == c.id }) {
            mentions.append(Mention(kind: c.kind, id: c.id, label: c.label))
        }
        suggestions = []
    }

    private func suggest() async {
        guard let q = query, let api = model.api else {
            suggestions = []
            return
        }
        try? await Task.sleep(for: .milliseconds(150))
        guard !Task.isCancelled else { return }
        suggestions = (try? await api.mentions(q.text, mail: q.sigil == "#")) ?? []
    }

    private func send() {
        let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty else { return }
        // Only tags still in the text count.
        let kept = mentions.filter { message.contains("\($0.kind.sigil)\($0.label)") }
        sending = true
        Task {
            do {
                try await onSend(message, kept)
                text = ""
                mentions = []
            } catch {
                self.error = error.localizedDescription
            }
            sending = false
        }
    }
}

private struct MentionSuggestions: View {
    let candidates: [MentionCandidate]
    let onPick: (MentionCandidate) -> Void

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                ForEach(candidates.prefix(8)) { c in
                    Button {
                        onPick(c)
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: icon(c.kind))
                                .foregroundStyle(.secondary)
                                .frame(width: 22)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(c.label).foregroundStyle(.primary)
                                if let detail = c.detail {
                                    Text(detail).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                                }
                            }
                            Spacer()
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .frame(maxHeight: 260)
        .fixedSize(horizontal: false, vertical: true)
        .glassEffect(.regular, in: .rect(cornerRadius: 18))
    }

    private func icon(_ kind: MentionKind) -> String {
        switch kind {
        case .person: "person.fill"
        case .event: "calendar"
        case .mailThread, .mailMessage: "envelope.fill"
        }
    }
}
