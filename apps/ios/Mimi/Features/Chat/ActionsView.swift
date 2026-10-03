import SwiftUI

/// What the assistant did (quiet lines) and asked to do (approval cards) in one reply.
/// The same grouping as the desktop's `features/chat/actions.tsx`.
struct ActionsView: View {
    let actions: [Action]

    /// Looking things up in memory is routine; it isn't shown.
    static let memoryReads: Set<String> = ["memory_search", "memory_read", "memory_list"]
    /// Changes to memory show as one quiet line each, with Undo.
    static let memoryWrites: Set<String> = ["memory_write", "memory_update", "memory_forget"]
    /// Reminders and routines set, changed or cancelled: also a quiet line with Undo.
    static let scheduleWrites: Set<String> = ["reminder_add", "routine_add", "schedule_change", "schedule_cancel"]
    /// Actions that ask first unless the user allowed them in Permissions. Done on their
    /// own, they show as a card with what was sent or added.
    static let mayBeAutomatic: Set<String> = [
        "mail_send", "calendar_add_event", "calendar_change_event", "calendar_delete_event", "calendar_send_invitations",
    ]

    var body: some View {
        let remembered = actions.filter { Self.memoryWrites.contains($0.tool) && $0.status == .done && $0.memoryRevision != nil }
        let scheduled = actions.filter { Self.scheduleWrites.contains($0.tool) && $0.status == .done && $0.scheduleRevision != nil }
        let automatic = actions.filter { !$0.requiresApproval && Self.mayBeAutomatic.contains($0.tool) }
        let reads = actions.filter { a in
            !a.requiresApproval && !Self.memoryReads.contains(a.tool) && !Self.memoryWrites.contains(a.tool)
                && !scheduled.contains(a) && !automatic.contains(a)
        }
        // An approved reminder becomes its Undo line.
        let asks = actions.filter { $0.requiresApproval && !scheduled.contains($0) }

        VStack(alignment: .leading, spacing: 10) {
            if !reads.isEmpty {
                FlowLayout(spacing: 14, lineSpacing: 4) {
                    ForEach(reads) { ReadStatus(action: $0) }
                }
            }
            ForEach(remembered) { a in
                UndoLine(action: a, icon: "bookmark.fill") { api in try await api.undoMemory(a.memoryRevision ?? 0) }
            }
            ForEach(scheduled) { a in
                UndoLine(action: a, icon: "bell.fill") { api in try await api.undoSchedule(a.scheduleRevision ?? 0) }
            }
            ForEach(automatic) { DecidedCard(action: $0, automatic: true) }
            ForEach(asks) { a in
                if a.status == .pendingApproval {
                    ApprovalCard(action: a)
                        .transition(.asymmetric(insertion: .scale(scale: 0.97).combined(with: .opacity), removal: .opacity))
                } else {
                    DecidedCard(action: a)
                }
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: actions.map(\.status))
    }
}

extension Action {
    var memoryRevision: Int? { output?["revision"]?.number.map { Int($0) } }
    var scheduleRevision: Int? { output?["schedule_revision"]?.number.map { Int($0) } }
    var undone: Bool { output?["undone"]?.bool == true }
    /// A page the user has to finish something at (saving an event in Google Calendar).
    var openURL: URL? {
        guard let s = output?["open_url"]?.string, s.hasPrefix("https://") else { return nil }
        return URL(string: s)
    }
    /// Something the user should know about a write, e.g. why guests weren't added.
    var note: String? { openURL == nil ? output?["note"]?.string : nil }
}

private struct ReadStatus: View {
    let action: Action

    var body: some View {
        switch action.status {
        case .done:
            Label((action.result ?? action.summary).upperFirst, systemImage: "checkmark")
                .labelStyle(QuietLabel(tint: .privateTone))
        case .failed:
            Label("Couldn't \(action.summary.lowerFirst)", systemImage: "xmark")
                .labelStyle(QuietLabel(tint: .danger))
                .foregroundStyle(Color.danger)
        default:
            Text("\(action.summary.lowerFirst)…")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .shimmering()
        }
    }
}

private struct QuietLabel: LabelStyle {
    var tint: Color
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 5) {
            configuration.icon.font(.caption.weight(.bold)).foregroundStyle(tint)
            configuration.title
        }
        .font(.subheadline)
        .foregroundStyle(.secondary)
    }
}

/// "Remembered that Sam is your brother · Undo".
private struct UndoLine: View {
    let action: Action
    let icon: String
    let undo: (MimiAPI) async throws -> Void
    @Environment(AppModel.self) private var model
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: icon).font(.caption)
            Text((action.result ?? action.summary).upperFirst)
                .strikethrough(action.undone)
            if action.undone {
                Text("· Undone")
            } else {
                Button("Undo") {
                    guard let api = model.api else { return }
                    busy = true
                    Task {
                        do { try await undo(api) } catch { self.error = error.localizedDescription }
                        busy = false
                    }
                }
                .font(.subheadline.weight(.medium))
                .disabled(busy)
            }
        }
        .font(.subheadline)
        .foregroundStyle(.secondary)
        .alert("Couldn't undo", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }
}

/// "Needs your OK": what the assistant wants to do, exactly as it will be done.
private struct ApprovalCard: View {
    let action: Action
    @Environment(AppModel.self) private var model
    @State private var busy: Choice?
    @State private var error: String?

    enum Choice { case approve, always, reject }

    var body: some View {
        let rows = ActionArguments.rows(tool: action.tool, arguments: action.arguments)
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "hand.raised.fill")
                    .font(.system(size: 17))
                    .foregroundStyle(Color.limeDeep)
                    .frame(width: 36, height: 36)
                    .background(Color.limeSoft, in: .rect(cornerRadius: 10))
                VStack(alignment: .leading, spacing: 2) {
                    Text("Needs your OK").font(.footnote.weight(.medium)).foregroundStyle(Color.limeDeep)
                    Text(action.summary).font(.headline)
                }
            }
            if !rows.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                        VStack(alignment: .leading, spacing: 1) {
                            Text(row.label).font(.footnote).foregroundStyle(.secondary)
                            Text(row.value).font(.callout).textSelection(.enabled)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(Color.subtle, in: .rect(cornerRadius: 14))
            }
            HStack(spacing: 10) {
                Button {
                    decide(.reject)
                } label: {
                    Text("Not now").frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .tint(.secondary)
                Button {
                    decide(.approve)
                } label: {
                    Group {
                        if busy == .approve { ProgressView().tint(.limeInk) } else { Text("Approve") }
                    }
                    .frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.lime)
            }
            .disabled(busy != nil)
            if let always = action.alwaysAllow {
                Button {
                    decide(.always)
                } label: {
                    HStack {
                        if busy == .always { ProgressView() }
                        Text(always)
                    }
                    .frame(maxWidth: .infinity)
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .disabled(busy != nil)
                .accessibilityHint("Approves this, and from now on does it without asking. You can change this in Settings, Permissions.")
            }
        }
        .padding(16)
        .background(.background, in: .rect(cornerRadius: 20))
        .shadow(color: .black.opacity(0.08), radius: 12, y: 4)
        .alert("Couldn't do that", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    private func decide(_ choice: Choice) {
        guard let api = model.api else { return }
        busy = choice
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        Task {
            do {
                switch choice {
                case .approve: try await api.approve(action.id)
                case .always: try await api.approve(action.id, always: true)
                case .reject: try await api.reject(action.id)
                }
            } catch {
                self.error = error.localizedDescription
                busy = nil
            }
        }
    }
}

/// An approval card after the decision: one compact line. `automatic` is for an action
/// the user let happen without asking; its details open from the line.
private struct DecidedCard: View {
    let action: Action
    var automatic = false
    @State private var open = false
    @Environment(\.openURL) private var openURL

    var body: some View {
        let rows = automatic ? ActionArguments.rows(tool: action.tool, arguments: action.arguments) : []
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                icon
                Group {
                    Text("\(text)\(automatic ? Text(" · automatic").foregroundStyle(.secondary) : Text(""))")
                }
                .foregroundStyle(action.status == .failed ? Color.danger : action.status == .done ? Color.primary : Color.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                if !rows.isEmpty {
                    Button(open ? "Hide" : "Details") { withAnimation { open.toggle() } }
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            if open {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                        VStack(alignment: .leading, spacing: 1) {
                            Text(row.label).font(.footnote).foregroundStyle(.secondary)
                            Text(row.value).font(.callout)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(12)
                .background(Color.subtle, in: .rect(cornerRadius: 12))
            }
            if action.status == .done, let note = action.note {
                Text(note).font(.subheadline).foregroundStyle(.secondary).padding(.leading, 30)
            }
            if action.status == .done, let url = action.openURL {
                Button {
                    openURL(url)
                } label: {
                    Label("Save in Google Calendar", systemImage: "arrow.up.right.square")
                }
                .buttonStyle(.lime)
            }
        }
        .font(.callout)
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .background(.background, in: .rect(cornerRadius: 14))
        .shadow(color: .black.opacity(0.05), radius: 6, y: 2)
    }

    private var text: String {
        switch action.status {
        case .done: (action.result ?? action.summary).upperFirst
        case .rejected: "Not done: \(action.summary.lowerFirst)"
        case .failed: "Couldn't do it\(action.error.map { ": \($0)" } ?? ".")"
        default: "On it: \(action.summary.lowerFirst)…"
        }
    }

    @ViewBuilder private var icon: some View {
        let (symbol, fg, bg): (String, Color, Color) = switch action.status {
        case .done: ("checkmark", .privateTone, .privateSoft)
        case .failed: ("exclamationmark", .danger, .dangerSoft)
        case .rejected: ("xmark", .secondary, .subtle)
        default: ("ellipsis", .secondary, .subtle)
        }
        Image(systemName: symbol)
            .font(.system(size: 10, weight: .bold))
            .foregroundStyle(fg)
            .frame(width: 20, height: 20)
            .background(bg, in: .circle)
    }
}

/// Lays out views left to right, wrapping onto new lines.
struct FlowLayout: Layout {
    var spacing: CGFloat = 8
    var lineSpacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, lineHeight: CGFloat = 0, maxX: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width { x = 0; y += lineHeight + lineSpacing; lineHeight = 0 }
            x += size.width + spacing
            maxX = max(maxX, x - spacing)
            lineHeight = max(lineHeight, size.height)
        }
        return CGSize(width: min(maxX, width), height: y + lineHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, lineHeight: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX { x = bounds.minX; y += lineHeight + lineSpacing; lineHeight = 0 }
            view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
        }
    }
}

/// A soft moving highlight over text that's still in progress ("Thinking", "reading…").
struct Shimmer: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var phase: CGFloat = -1

    func body(content: Content) -> some View {
        if reduceMotion {
            content
        } else {
            content
                .overlay {
                    GeometryReader { geo in
                        LinearGradient(colors: [.clear, .white.opacity(0.7), .clear], startPoint: .leading, endPoint: .trailing)
                            .frame(width: geo.size.width * 0.6)
                            .offset(x: phase * geo.size.width * 1.6)
                    }
                    .mask(content)
                    .allowsHitTesting(false)
                }
                .onAppear {
                    withAnimation(.linear(duration: 1.4).repeatForever(autoreverses: false)) { phase = 1 }
                }
        }
    }
}

extension View {
    func shimmering() -> some View { modifier(Shimmer()) }
}
