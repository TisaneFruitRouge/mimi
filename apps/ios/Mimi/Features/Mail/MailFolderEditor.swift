import SwiftUI

/// Makes or edits a smart folder: a name, how it looks, and in the user's words what goes
/// in it. Folders exist only in Mimi; nothing changes in the mail account.
struct MailFolderEditor: View {
    let folder: MailFolder?
    let overview: MailOverview
    @Environment(AppModel.self) private var model
    @Environment(MailStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var description: String
    @State private var icon: String
    @State private var color: String
    @State private var busy = false
    @State private var error: String?

    init(folder: MailFolder?, overview: MailOverview) {
        self.folder = folder
        self.overview = overview
        _name = State(initialValue: folder?.name ?? "")
        _description = State(initialValue: folder?.description ?? "")
        _icon = State(initialValue: folder?.icon ?? "sparkles")
        _color = State(initialValue: folder?.color ?? "violet")
    }

    private var valid: Bool {
        !name.trimmingCharacters(in: .whitespaces).isEmpty && !description.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    HStack(spacing: 14) {
                        MailFolderTile(icon: icon, color: color, size: 44)
                            .animation(.spring(response: 0.3, dampingFraction: 0.8), value: icon + color)
                        TextField("Receipts", text: $name)
                            .font(.title3.weight(.semibold))
                            .onChange(of: name) { _, v in if v.count > 60 { name = String(v.prefix(60)) } }
                            .accessibilityLabel("Name")
                    }
                    .padding(.vertical, 4)
                } footer: {
                    Text("Say what belongs in it, the way you'd tell a person. Your mail is filed in the background, newest first. Folders only exist here: nothing changes in your mail account.")
                }

                Section("What goes in it?") {
                    TextField("Receipts and invoices from shops and online services", text: $description, axis: .vertical)
                        .lineLimit(3...6)
                        .onChange(of: description) { _, v in if v.count > 400 { description = String(v.prefix(400)) } }
                        .accessibilityLabel("What goes in it")
                    if let folder, description.trimmingCharacters(in: .whitespaces) != folder.description {
                        Text("A new description files your mail again. Conversations you moved yourself stay put.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }

                Section("Colour") {
                    HStack(spacing: 0) {
                        ForEach(MailFolderLooks.colors, id: \.name) { c in
                            Button {
                                color = c.name
                            } label: {
                                Circle()
                                    .fill(Color(hex: c.ink))
                                    .frame(width: 26, height: 26)
                                    .padding(3)
                                    .overlay {
                                        if c.name == color {
                                            Circle().strokeBorder(Color(hex: c.ink), lineWidth: 2)
                                        }
                                    }
                                    .frame(maxWidth: .infinity)
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(c.label)
                            .accessibilityAddTraits(c.name == color ? .isSelected : [])
                        }
                    }
                    .padding(.vertical, 4)
                }

                Section("Icon") {
                    LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 6), spacing: 6) {
                        ForEach(MailFolderLooks.icons, id: \.name) { i in
                            let on = i.name == icon
                            Button {
                                icon = i.name
                            } label: {
                                Image(systemName: i.symbol)
                                    .font(.system(size: 17, weight: .medium))
                                    .foregroundStyle(on ? MailFolderLooks.ink(color) : Color.secondary)
                                    .frame(maxWidth: .infinity, minHeight: 40)
                                    .background(on ? MailFolderLooks.soft(color) : .clear, in: .rect(cornerRadius: 10))
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(i.name.replacingOccurrences(of: "-", with: " "))
                            .accessibilityAddTraits(on ? .isSelected : [])
                        }
                    }
                    .padding(.vertical, 4)
                }

                Section {
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 6) {
                            Text(overview.sorter == .jev ? "Filed by Jev" : "Filed by your model")
                            if let l = overview.sorterLocality { LocalityBadge(locality: l) }
                        }
                        if overview.sorter == .jev {
                            Text("Newsletters are sent to TypeSafe too, so they can be filed. Suspicious mail never is.")
                        }
                    }
                }
            }
            .navigationTitle(folder == nil ? "New smart folder" : "Edit smart folder")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", systemImage: "xmark") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        Task { await save() }
                    } label: {
                        if busy { ProgressView() } else { Text(folder == nil ? "Make folder" : "Save") }
                    }
                    .disabled(!valid || busy)
                }
            }
            .alert("Couldn't save the folder", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
    }

    private func save() async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        let input = MailFolderInput(
            name: name.trimmingCharacters(in: .whitespaces),
            description: description.trimmingCharacters(in: .whitespacesAndNewlines),
            icon: icon,
            color: color
        )
        do {
            if let folder {
                try await api.updateMailFolder(folder.id, input)
                store.say("Folder saved")
            } else {
                try await api.createMailFolder(input)
                store.say("“\(input.name)” is being filled")
            }
            store.changed()
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
