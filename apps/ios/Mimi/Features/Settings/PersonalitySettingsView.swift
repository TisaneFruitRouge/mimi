import SwiftUI

/// Starting points for the assistant's personality (`features/personality/presets.ts`).
/// "Default" is the empty text: the assistant keeps the voice it always had.
enum PersonalityPresets {
    struct Preset: Identifiable, Hashable {
        var id: String
        var label: String
        var text: String
    }

    static let all: [Preset] = [
        Preset(id: "default", label: "Default", text: ""),
        Preset(id: "warm", label: "Warm & friendly", text:
            "Warm and encouraging, like a friend who happens to be well organised. Relaxed, everyday "
            + "language. Take an interest in how things went, but don't gush, and go easy on exclamation marks."),
        Preset(id: "concise", label: "Calm & concise", text:
            "Calm and to the point. Give the answer first, in as few words as it takes. No small talk, no "
            + "filler, no repeating my question back. Use a short list only when there are several steps or options."),
        Preset(id: "playful", label: "Playful", text:
            "Light-hearted, with a dry sense of humour. A quick joke or wry aside is welcome when the moment "
            + "fits, never when the news is bad or the task is serious. Still get things right and keep it short."),
        Preset(id: "professional", label: "Professional", text:
            "Polished and precise, like a good executive assistant. Courteous and neutral, no jokes or emoji. "
            + "Clear structure, exact dates and numbers, and say plainly when something is uncertain."),
        Preset(id: "straight", label: "Straight talker", text:
            "Direct and honest. Tell me what you actually think, including when I'm wrong or a plan has a "
            + "hole in it. No sugar-coating and no hedging, but stay kind."),
    ]

    /// Mirrors `PERSONALITY_LIMIT` and `INSTRUCTIONS_LIMIT` in crates/protocol/src/settings.rs.
    static let personalityLimit = 600
    static let instructionsLimit = 1000
    static let nameLimit = 40

    static func matching(_ text: String) -> Preset? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return all.first { $0.text == t }
    }
}

/// Settings › Personality: the assistant's name, who it is and how it talks, and the
/// user's standing instructions. Saved shortly after typing stops.
struct PersonalitySettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var name = ""
    @State private var personality = ""
    @State private var instructions = ""
    @State private var loaded = false
    @State private var saving: Set<String> = []
    @State private var savedAt: Date?
    @State private var error: String?
    @State private var notice: UndoNotice?
    @FocusState private var focus: Field?

    private enum Field: Hashable { case name, personality, instructions }

    var body: some View {
        Form {
            Section {
                VStack(spacing: 8) {
                    AssistantAvatar(size: 76, mood: focus == nil ? .idle : .happy)
                    Text("Who \(model.assistantName) is, how it talks to you, and what it should always keep in mind.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }

            Section {
                TextField("Your assistant's name", text: $name)
                    .focused($focus, equals: .name)
                    .submitLabel(.done)
                    .onSubmit { Task { await save("assistant_name", name) } }
            } header: {
                Text("Name")
            }

            Section {
                FlowLayout(spacing: 8) {
                    ForEach(PersonalityPresets.all) { p in
                        presetChip(p)
                    }
                }
                .padding(.vertical, 6)
                TextField(
                    "Helpful, direct and warm: that's how \(model.assistantName) talks now. Pick a starting point above, or describe the personality you'd like in your own words.",
                    text: $personality, axis: .vertical
                )
                .lineLimit(4...12)
                .focused($focus, equals: .personality)
                .accessibilityLabel("Personality")
            } header: {
                Text("Personality")
            } footer: {
                CountNote(count: personality.count, limit: PersonalityPresets.personalityLimit)
            }

            Section {
                TextField(
                    "Answer in French unless I write in English.\nSign my emails with just my first name.\nI'm vegetarian, so keep that in mind for recipes and restaurants.",
                    text: $instructions, axis: .vertical
                )
                .lineLimit(5...14)
                .focused($focus, equals: .instructions)
                .accessibilityLabel("Custom instructions")
            } header: {
                Text("Custom instructions")
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    CountNote(count: instructions.count, limit: PersonalityPresets.instructionsLimit)
                    Text("\(model.assistantName) follows these in every conversation, on Telegram and in routines, and when it drafts an email for you. They can't turn off anything in Permissions. When you use a model in the cloud, they're sent along with your messages, so leave out passwords and other secrets.")
                }
            }
        }
        .navigationTitle("Personality")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                if !saving.isEmpty {
                    Text("Saving…").font(.footnote).foregroundStyle(.secondary)
                } else if savedAt != nil {
                    Text("Saved").font(.footnote).foregroundStyle(.secondary)
                }
            }
        }
        .onAppear(perform: loadFromSettings)
        .onChange(of: model.settingsJSON) { _, _ in
            // Changed elsewhere (the computer): follow, unless it's being edited here.
            if focus == nil && saving.isEmpty { loadFromSettings() }
        }
        .onChange(of: personality) { _, v in
            if v.count > PersonalityPresets.personalityLimit { personality = String(v.prefix(PersonalityPresets.personalityLimit)) }
        }
        .onChange(of: instructions) { _, v in
            if v.count > PersonalityPresets.instructionsLimit { instructions = String(v.prefix(PersonalityPresets.instructionsLimit)) }
        }
        .onChange(of: name) { _, v in
            if v.count > PersonalityPresets.nameLimit { name = String(v.prefix(PersonalityPresets.nameLimit)) }
        }
        .onChange(of: focus) { old, _ in
            // Leaving a field saves it right away.
            switch old {
            case .name: Task { await save("assistant_name", name) }
            case .personality: Task { await save("personality", personality) }
            case .instructions: Task { await save("custom_instructions", instructions) }
            case nil: break
            }
        }
        // Saved shortly after typing stops.
        .task(id: personality) { await debounced("personality", personality) }
        .task(id: instructions) { await debounced("custom_instructions", instructions) }
        .onDisappear {
            Task {
                await save("assistant_name", name)
                await save("personality", personality)
                await save("custom_instructions", instructions)
            }
        }
        .task(id: savedAt) {
            guard savedAt != nil else { return }
            try? await Task.sleep(for: .seconds(2))
            if !Task.isCancelled { withAnimation { savedAt = nil } }
        }
        .undoNotice($notice)
        .problemAlert($error, title: "Couldn't save")
    }

    private func presetChip(_ p: PersonalityPresets.Preset) -> some View {
        let selected = PersonalityPresets.matching(personality)?.id == p.id
        return Button {
            choose(p.text)
        } label: {
            HStack(spacing: 4) {
                if selected { Image(systemName: "checkmark").font(.caption.weight(.bold)) }
                Text(p.label)
            }
            .font(.subheadline.weight(.medium))
            .padding(.horizontal, 12)
            .frame(height: 32)
            .foregroundStyle(Color.ink)
            .background(selected ? Color(.tertiarySystemFill) : Color(.systemBackground), in: .capsule)
            .overlay(Capsule().stroke(Color.black.opacity(selected ? 0 : 0.12), lineWidth: 0.5))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func choose(_ text: String) {
        let before = personality
        guard before.trimmingCharacters(in: .whitespacesAndNewlines) != text else { return }
        personality = text
        Task { await save("personality", text) }
        // Replacing something the user wrote themselves can be undone.
        let trimmed = before.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty && PersonalityPresets.matching(before) == nil {
            notice = UndoNotice(text: "Personality replaced") {
                personality = before
                await save("personality", before)
            }
        }
    }

    private func loadFromSettings() {
        name = model.assistantName
        personality = model.setting("personality")?.string ?? ""
        instructions = model.setting("custom_instructions")?.string ?? ""
        loaded = true
    }

    private func debounced(_ key: String, _ text: String) async {
        guard loaded else { return }
        try? await Task.sleep(for: .milliseconds(800))
        guard !Task.isCancelled else { return }
        await save(key, text)
    }

    /// Saves one field, unless it's what the computer already has. An empty name keeps
    /// the old one.
    private func save(_ key: String, _ text: String) async {
        guard loaded else { return }
        let current = key == "assistant_name" ? model.assistantName : (model.setting(key)?.string ?? "")
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if key == "assistant_name" && trimmed.isEmpty {
            name = model.assistantName
            return
        }
        guard trimmed != current.trimmingCharacters(in: .whitespacesAndNewlines) else { return }
        saving.insert(key)
        defer { saving.remove(key) }
        do {
            try await model.updateSettings([key: .string(key == "assistant_name" ? trimmed : text)])
            savedAt = .now
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// A quiet character count that shows as the limit gets close.
private struct CountNote: View {
    let count: Int
    let limit: Int

    var body: some View {
        if Double(count) >= Double(limit) * 0.8 {
            Text("\(count) / \(limit)")
                .monospacedDigit()
                .foregroundStyle(count >= limit ? Color.danger : .secondary)
        }
    }
}
