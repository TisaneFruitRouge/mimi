import SwiftUI

/// "Add someone": a name, a nickname, and any ways to reach them. Kept on the computer.
struct AddPersonSheet: View {
    let onAdded: (Person) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var nickname = ""
    @State private var handles: [NewHandle] = []
    @State private var addingHandle = false
    @State private var busy = false
    @State private var error: String?
    @FocusState private var nameFocused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name)
                        .textContentType(.name)
                        .focused($nameFocused)
                    TextField("Nickname (optional, like Mum or Sammy)", text: $nickname)
                } footer: {
                    Text("Kept on your computer. \(model.assistantName) can then mention and reach them.")
                }
                Section("Ways to reach them") {
                    ForEach(Array(handles.enumerated()), id: \.offset) { i, h in
                        HStack(spacing: 12) {
                            ChannelGlyph(channel: h.channel, size: 15).frame(width: 24)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(h.value)
                                Text([h.label.map(handleLabelText), h.channel.label].compactMap { $0 }.joined(separator: " · "))
                                    .font(.footnote).foregroundStyle(.secondary)
                            }
                        }
                        .swipeActions {
                            Button("Remove", systemImage: "trash", role: .destructive) { handles.remove(at: i) }
                        }
                    }
                    Button { addingHandle = true } label: {
                        Label("Add a phone, email or username", systemImage: "plus")
                    }
                }
                if let error {
                    Section { Text(error).foregroundStyle(Color.danger).font(.subheadline) }
                }
            }
            .navigationTitle("Add someone")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { Task { await add() } }
                        .disabled(busy || name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .sheet(isPresented: $addingHandle) {
                HandleSheet(title: "A way to reach them", submitLabel: "Add") { h in handles.append(h) }
            }
            .onAppear { nameFocused = true }
        }
    }

    private func add() async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            let nick = nickname.trimmingCharacters(in: .whitespaces)
            let person = try await api.addPerson(NewPerson(name: name, nickname: nick.isEmpty ? nil : nick, handles: handles))
            dismiss()
            onAdded(person)
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// Changing someone's name and nickname.
struct NameSheet: View {
    let person: Person
    let onSaved: (Person) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var nickname = ""
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name)
                    TextField("Nickname (optional)", text: $nickname)
                } footer: {
                    if let error { Text(error).foregroundStyle(Color.danger) }
                }
            }
            .navigationTitle("Change name")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { Task { await save() } }
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear {
                name = person.name
                nickname = person.nickname ?? ""
            }
        }
        .presentationDetents([.medium])
    }

    private func save() async {
        do {
            guard let api = model.api else { return }
            onSaved(try await api.updatePerson(person.id, PersonUpdate(name: name, nickname: nickname)))
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// One way to reach someone: how, the number or address, and an optional label.
struct HandleSheet: View {
    let title: String
    let submitLabel: String
    var initial: NewHandle?
    let onSubmit: (NewHandle) async throws -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var channel: Channel = .phone
    @State private var value = ""
    @State private var label = ""
    /// Typing a label of one's own.
    @State private var custom = false
    @State private var busy = false
    @State private var error: String?
    @FocusState private var valueFocused: Bool

    init(title: String, submitLabel: String, initial: NewHandle? = nil, onSubmit: @escaping (NewHandle) async throws -> Void) {
        self.title = title
        self.submitLabel = submitLabel
        self.initial = initial
        self.onSubmit = onSubmit
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("How", selection: $channel) {
                        ForEach(Channel.addable, id: \.self) { c in
                            Label(c.label, systemImage: c.symbol).tag(c)
                        }
                    }
                    TextField(channel.placeholder.isEmpty ? "Number, address or username" : channel.placeholder, text: $value)
                        .keyboardType(keyboard)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($valueFocused)
                }
                Section("Label") {
                    FlowLayout(spacing: 8) {
                        chip("None", active: !custom && label.isEmpty) { custom = false; label = "" }
                        ForEach(channel.labelChoices, id: \.self) { c in
                            chip(handleLabelText(c), active: !custom && label == c) { custom = false; label = c }
                        }
                        chip("Other…", active: custom) {
                            custom = true
                            if channel.labelChoices.contains(label) { label = "" }
                        }
                    }
                    .padding(.vertical, 4)
                    if custom {
                        TextField("Like school or holiday home", text: $label)
                    }
                }
                if let error {
                    Section { Text(error).foregroundStyle(Color.danger).font(.subheadline) }
                }
            }
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(submitLabel) { Task { await submit() } }
                        .disabled(busy || value.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onChange(of: channel) { _, next in
                if !custom && !label.isEmpty && !next.labelChoices.contains(label) { label = "" }
            }
            .onAppear {
                if let initial {
                    channel = initial.channel
                    value = initial.value
                    label = initial.label ?? ""
                    custom = !label.isEmpty && !initial.channel.labelChoices.contains(label)
                }
                valueFocused = true
            }
        }
        .presentationDetents([.medium, .large])
    }

    private var keyboard: UIKeyboardType {
        switch channel {
        case .phone, .signal, .whatsapp: .phonePad
        case .email: .emailAddress
        default: .default
        }
    }

    private func chip(_ text: String, active: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(text)
                .font(.subheadline.weight(.medium))
                .padding(.horizontal, 12)
                .frame(height: 30)
                .foregroundStyle(active ? Color.white : Color.ink)
                .background(active ? Color.ink : Color(.tertiarySystemFill), in: .capsule)
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(active ? .isSelected : [])
    }

    private func submit() async {
        busy = true
        defer { busy = false }
        error = nil
        let trimmed = label.trimmingCharacters(in: .whitespaces)
        do {
            try await onSubmit(NewHandle(channel: channel, value: value, label: trimmed.isEmpty ? nil : trimmed))
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
