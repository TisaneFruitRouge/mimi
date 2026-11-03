import SwiftUI

/// Settings › Memory: what the assistant knows about the user and the people in their
/// life, all of it editable. Kept on the computer.
struct MemorySettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var overview: MemoryOverview?
    @State private var pulls: [ModelPull] = []
    @State private var busy = false
    @State private var adding = false
    @State private var confirmForget = false
    @State private var notice: UndoNotice?
    @State private var error: String?

    var body: some View {
        List {
            if model.modelLocality == .cloud {
                Section {
                    Label {
                        Text("You're using \(cloudName), a cloud service. The parts of your memory that matter to a message are sent to it along with the message.")
                            .font(.subheadline)
                    } icon: {
                        Image(systemName: "cloud.fill")
                    }
                    .foregroundStyle(Color.cloudTone)
                    .listRowBackground(Color.cloudSoft)
                }
            }

            if let o = overview {
                Section {
                    ExplainedRow(systemImage: "checkmark.shield.fill", tint: .privateSoft, foreground: .privateTone,
                                 title: "Learn from conversations",
                                 detail: o.learning
                                    ? "\(model.assistantName) notices lasting things you tell it, like who's who in your life, and remembers them."
                                    : "Paused. Nothing new is remembered, but \(model.assistantName) still uses what it already knows.") {
                        Toggle("Learn from conversations", isOn: Binding(get: { o.learning }, set: { on in Task { await setLearning(on) } }))
                            .labelsHidden()
                            .disabled(busy)
                    }
                    if let s = o.semantic {
                        meaning(s)
                    }
                }

                Section {
                    NavigationLink {
                        ProfileEditorView(profile: o.profile, limit: o.profileLimit)
                    } label: {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(o.profile.isEmpty ? "Nothing yet" : o.profile)
                                .font(.subheadline)
                                .foregroundStyle(o.profile.isEmpty ? .secondary : .primary)
                                .lineLimit(4)
                        }
                        .padding(.vertical, 2)
                    }
                } header: {
                    Text("About you")
                } footer: {
                    Text("The essentials \(model.assistantName) always keeps in mind. Details live in the notes below.")
                }

                if o.notes.isEmpty {
                    Section("Notes") {
                        ExplainedRow(systemImage: "book.closed", tint: Color(.tertiarySystemFill), foreground: .secondary,
                                     title: "Nothing remembered yet",
                                     detail: "Tell \(model.assistantName) about the people in your life, your routines and what you like. It keeps notes here.")
                    }
                } else {
                    ForEach(MemoryFolders.groups(o.notes), id: \.id) { group in
                        Section(group.label) {
                            ForEach(group.notes) { n in
                                NavigationLink {
                                    MemoryNoteView(path: n.path) { deleted in noteDeleted(deleted) }
                                } label: {
                                    noteRow(n)
                                }
                                .swipeActions {
                                    Button("Delete", systemImage: "trash") { Task { await delete(n.path) } }
                                        .tint(.red)
                                }
                            }
                        }
                    }
                }

                Section {
                    Button("Forget everything", role: .destructive) { confirmForget = true }
                        .disabled(o.notes.isEmpty && o.profile.isEmpty)
                } footer: {
                    Text("Deletes all memories. Your conversations are kept.")
                }
            } else {
                Section { ProgressView().frame(maxWidth: .infinity) }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Memory")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Add a note", systemImage: "plus") { adding = true }
            }
        }
        .task(id: model.revision("memory_changed")) { await load() }
        .task(id: model.revision("model_pull")) {
            if overview?.semantic?.enabled == true, let list = try? await model.api?.pulls() { pulls = list }
        }
        .refreshable { await load() }
        .sheet(isPresented: $adding) { AddNoteSheet() }
        .alert("Forget everything?", isPresented: $confirmForget) {
            Button("Cancel", role: .cancel) {}
            Button("Forget everything", role: .destructive) { Task { await forgetAll() } }
        } message: {
            Text("\(model.assistantName) will no longer know anything it learned about you or the people in your life. This can't be undone.")
        }
        .undoNotice($notice)
        .problemAlert($error)
    }

    private var cloudName: String {
        let id = model.settings.defaultModel?.providerId
        return model.providers.first { $0.id == id }?.name ?? "a cloud model"
    }

    private func noteRow(_ n: MemoryNoteSummary) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 5) {
                Text(n.title)
                if let who = n.subjectName {
                    Image(systemName: "person.crop.circle")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .accessibilityLabel("About \(who)")
                }
            }
            if !n.preview.isEmpty {
                Text(MemoryFolders.forYou(n.preview))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 1)
    }

    /// "Understands meaning": finding notes by what they're about, with a small local model.
    @ViewBuilder
    private func meaning(_ s: MemorySemantic) -> some View {
        let pull = pulls.first { $0.providerId == s.providerId && $0.model == s.model && $0.state == .running }
        let size = formatBytes(s.downloadBytes)
        let detail: String = {
            if !s.available { return "Needs Mimi's built-in model runtime or Ollama on your computer." }
            if !s.enabled {
                return "Finds notes by what they're about, not only their words: “my sibling” finds your sister's note, in any language. A one-time \(size) download to your computer."
            }
            if let pull { return "Downloading to your computer… \(Int((pull.fraction ?? 0) * 100))% of \(size)." }
            if !s.installed { return "The download stopped before it finished." }
            if let e = s.error { return e }
            if s.indexed < s.total { return "Reading your notes… \(s.indexed) of \(s.total)." }
            return "On. \(model.assistantName) finds notes by their meaning, in any language. Nothing leaves your devices."
        }()
        ExplainedRow(systemImage: "sparkles", tint: .privateSoft, foreground: .privateTone,
                     title: "Understands meaning", detail: detail) {
            Toggle("Understands meaning", isOn: Binding(get: { s.enabled }, set: { on in Task { await setSemantic(on) } }))
                .labelsHidden()
                .disabled(busy || !s.available)
        }
        if s.enabled, let pull {
            ProgressView(value: pull.fraction ?? 0)
                .tint(Color.lime)
        }
        if s.enabled && s.available && !s.installed && pull == nil {
            Button("Try the download again") { Task { await setSemantic(true) } }
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            overview = try await api.memory()
            if overview?.semantic?.enabled == true { pulls = (try? await api.pulls()) ?? [] }
        } catch {
            if overview == nil { self.error = error.localizedDescription }
        }
    }

    private func setLearning(_ on: Bool) async {
        busy = true
        defer { busy = false }
        do {
            try await model.api?.setMemoryLearning(on)
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func setSemantic(_ on: Bool) async {
        busy = true
        defer { busy = false }
        do {
            let s = try await model.api?.setMemorySemantic(on)
            overview?.semantic = s
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func delete(_ path: String) async {
        guard let api = model.api else { return }
        do {
            let note = try await api.memoryNote(path)
            try await api.deleteMemoryNote(path)
            noteDeleted(note)
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Deleting a note can be undone: it's written back as it was.
    private func noteDeleted(_ note: MemoryNote) {
        notice = UndoNotice(text: "“\(note.title)” was forgotten") { [model] in
            try await model.api?.saveMemoryNote(note.path, title: note.title, body: note.body)
        }
    }

    private func forgetAll() async {
        do {
            try await model.api?.forgetEverything()
            notice = UndoNotice(text: "Everything was forgotten")
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// "About you": the short summary that's always given to the assistant.
struct ProfileEditorView: View {
    let profile: String
    let limit: Int
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        Form {
            Section {
                TextField("Nothing yet. \(model.assistantName) fills this in as you talk, or you can write a few lines about yourself.",
                          text: $text, axis: .vertical)
                    .lineLimit(8...20)
            } footer: {
                Text("\(text.count) of \(limit) characters")
                    .monospacedDigit()
                    .foregroundStyle(text.count > limit ? Color.danger : .secondary)
            }
        }
        .navigationTitle("About you")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") { Task { await save() } }
                    .disabled(busy || text == profile || text.count > limit)
            }
        }
        .onAppear { text = profile }
        .problemAlert($error)
    }

    private func save() async {
        busy = true
        defer { busy = false }
        do {
            try await model.api?.saveMemoryProfile(text)
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// One note: its title and what it says, editable, with who wrote it last.
struct MemoryNoteView: View {
    let path: String
    var onDeleted: ((MemoryNote) -> Void)?
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var note: MemoryNote?
    @State private var title = ""
    @State private var text = ""
    @State private var confirmDelete = false
    @State private var busy = false
    @State private var notice: UndoNotice?
    @State private var error: String?

    private var dirty: Bool { note != nil && (title != note!.title || text != note!.body) }

    var body: some View {
        Form {
            if let n = note {
                Section {
                    TextField("Title", text: $title)
                        .font(.title3.weight(.semibold))
                } footer: {
                    HStack(spacing: 6) {
                        if let who = n.subjectName {
                            Label("About \(who)", systemImage: "person.crop.circle")
                        }
                        Text("\(n.source.label) · \(noteDay(n.updatedAt))")
                    }
                }
                Section("What \(model.assistantName) remembers") {
                    TextField("What to remember", text: $text, axis: .vertical)
                        .lineLimit(6...30)
                }
                Section {
                    Button("Delete note", role: .destructive) { confirmDelete = true }
                }
            } else {
                Section { ProgressView().frame(maxWidth: .infinity) }
            }
        }
        .navigationTitle(note?.title ?? "Note")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") { Task { await save() } }
                    .disabled(!dirty || busy)
            }
        }
        .task { await load() }
        .alert("Delete “\(note?.title ?? "")”?", isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { Task { await delete() } }
        } message: {
            Text("\(model.assistantName) will forget what this note says.")
        }
        .undoNotice($notice)
        .problemAlert($error)
    }

    private func load() async {
        do {
            guard let n = try await model.api?.memoryNote(path) else { return }
            note = n
            title = n.title
            text = n.body
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func save() async {
        guard let api = model.api, let before = note else { return }
        busy = true
        defer { busy = false }
        do {
            let saved = try await api.saveMemoryNote(path, title: title, body: text)
            note = saved
            title = saved.title
            text = saved.body
            notice = UndoNotice(text: "Saved") {
                let back = try await api.saveMemoryNote(before.path, title: before.title, body: before.body)
                note = back
                title = back.title
                text = back.body
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func delete() async {
        guard let n = note else { return }
        do {
            try await model.api?.deleteMemoryNote(path)
            onDeleted?(n)
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func noteDay(_ ms: Int64) -> String {
        let days = Int((Date.now.timeIntervalSince1970 * 1000 - Double(ms)) / 86_400_000)
        if days <= 0 { return "today" }
        if days == 1 { return "yesterday" }
        if days < 30 { return "\(days) days ago" }
        return Date(timeIntervalSince1970: Double(ms) / 1000).formatted(date: .long, time: .omitted)
    }
}

/// "Add a note": something the assistant should know.
struct AddNoteSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var folder = "people"
    @State private var title = ""
    @State private var text = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Kind", selection: $folder) {
                        ForEach(MemoryFolders.all, id: \.id) { f in Text(f.label).tag(f.id) }
                    }
                    TextField(folder == "people" ? "Name, like Sam" : "Title, like Mornings", text: $title)
                }
                Section("What to remember") {
                    TextField("- My brother\n- Birthday on 3 May\n- Loves climbing", text: $text, axis: .vertical)
                        .lineLimit(5...14)
                }
                if let error {
                    Section { Text(error).foregroundStyle(Color.danger).font(.subheadline) }
                }
            }
            .navigationTitle("Add a note")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { Task { await add() } }
                        .disabled(busy || title.trimmingCharacters(in: .whitespaces).isEmpty
                                  || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }

    private func add() async {
        let name = title.trimmingCharacters(in: .whitespaces)
        busy = true
        defer { busy = false }
        do {
            // The computer turns the name into a tidy path (people/léa.md).
            try await model.api?.saveMemoryNote("\(folder)/\(name)", title: name, body: text)
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
