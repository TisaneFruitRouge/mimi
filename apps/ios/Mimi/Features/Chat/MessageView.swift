import SwiftUI

/// One message. The user's are grey bubbles on the right; replies are plain text with
/// their actions in place, no avatar.
struct MessageView: View {
    let message: Message

    var body: some View {
        if message.role == .user {
            HStack {
                Spacer(minLength: 48)
                MentionText(text: message.content, mentions: message.mentions)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(Color.userBubble, in: UnevenRoundedRectangle(
                        topLeadingRadius: 20, bottomLeadingRadius: 20, bottomTrailingRadius: 6, topTrailingRadius: 20))
                    .contextMenu {
                        Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = message.content }
                    }
            }
        } else {
            AssistantMessage(message: message)
        }
    }
}

private struct AssistantMessage: View {
    let message: Message
    @State private var showReasoning = false

    var body: some View {
        let streaming = message.status == .streaming
        let acting = message.actions.contains { [.pendingApproval, .approved, .running].contains($0.status) }
        VStack(alignment: .leading, spacing: 10) {
            if streaming && message.content.isEmpty && !acting {
                ThinkingStatus()
            }
            if !message.reasoning.isEmpty {
                DisclosureGroup(isExpanded: $showReasoning) {
                    Text(message.reasoning)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } label: {
                    Text(streaming && message.content.isEmpty ? "Thinking it through…" : "How it thought about it")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .tint(.secondary)
            }
            ForEach(Array(segments.enumerated()), id: \.offset) { _, segment in
                switch segment {
                case .text(let text): MarkdownText(text: text)
                case .actions(let actions): ActionsView(actions: actions)
                }
            }
            if message.status == .error {
                Label(message.error ?? "Something went wrong while writing this reply.", systemImage: "exclamationmark.circle")
                    .font(.callout)
                    .foregroundStyle(Color.danger)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.dangerSoft, in: .rect(cornerRadius: 14))
            }
            if !streaming {
                HStack(spacing: 8) {
                    // Only cloud use is worth pointing out: private is the default.
                    if message.locality == .cloud {
                        Label("Answered by a cloud service", systemImage: "cloud.fill")
                            .foregroundStyle(Color.cloudTone)
                    }
                    if message.status == .cancelled { Text("Stopped") }
                    if message.status == .interrupted { Text("This reply was cut off") }
                }
                .font(.footnote)
                .foregroundStyle(.secondary)
            }
        }
        .contextMenu {
            if !message.content.isEmpty {
                Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = message.content }
                ShareLink(item: message.content)
            }
        }
    }

    enum Segment {
        case text(String)
        case actions([Action])
    }

    /// The reply's text with its actions placed where they happened.
    private var segments: [Segment] {
        let chars = Array(message.content)
        let actions = message.actions.sorted { $0.contentOffset < $1.contentOffset }
        var out: [Segment] = []
        var at = 0
        for a in actions {
            let offset = min(a.contentOffset, chars.count)
            if offset > at {
                let text = String(chars[at..<offset])
                if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { out.append(.text(text)) }
            }
            at = max(at, offset)
            if case .actions(var list)? = out.last {
                list.append(a)
                out[out.count - 1] = .actions(list)
            } else {
                out.append(.actions([a]))
            }
        }
        if at < chars.count {
            let rest = String(chars[at...])
            if !rest.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { out.append(.text(rest)) }
        }
        return out
    }
}

/// Shown before the first words arrive: the assistant, small, mulling it over. A status,
/// not an avatar: it goes away as soon as the reply starts.
struct ThinkingStatus: View {
    var body: some View {
        HStack(spacing: 8) {
            AssistantAvatar(size: 28, mood: .thinking)
            Text("Thinking").font(.callout).foregroundStyle(.secondary).shimmering()
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Thinking")
    }
}

/// A message's text with its @ and # tags shown as tags.
struct MentionText: View {
    let text: String
    let mentions: [Mention]

    var body: some View {
        Text(attributed)
    }

    private var attributed: AttributedString {
        var out = AttributedString(text)
        for m in mentions {
            let tag = "\(m.kind.sigil)\(m.label)"
            var search = out.startIndex
            while let range = out[search...].range(of: tag) {
                out[range].foregroundColor = m.kind.sigil == "#" ? .networkTone : .limeDeep
                out[range].font = .body.weight(.medium)
                search = range.upperBound
            }
        }
        return out
    }
}
