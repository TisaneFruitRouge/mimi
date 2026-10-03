import SwiftUI

/// Settings › Privacy › Email: whether new mail is sorted in the background, and by what:
/// the user's own model (the default), or Jev, a cloud decision model, with their own
/// TypeSafe key. The same choices as the desktop's Privacy page.
struct MailPrivacyView: View {
    @Environment(AppModel.self) private var model
    @State private var overview: MailOverview?
    @State private var busy = false
    @State private var askingKey = false
    @State private var confirmRemove = false
    @State private var error: String?
    @State private var note: String?

    var body: some View {
        Form {
            if let o = overview {
                if o.accounts.isEmpty {
                    Section {
                        Text("Connect an email account first, in Settings › Connections.")
                            .foregroundStyle(.secondary)
                    }
                } else {
                    sections(o)
                }
            } else {
                HStack { Spacer(); ProgressView(); Spacer() }
                    .listRowBackground(Color.clear)
            }
        }
        .navigationTitle("Email")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: model.revision("mail_changed")) { await load() }
        .sheet(isPresented: $askingKey) {
            JevKeySheet {
                askingKey = false
                Task {
                    await set(["mail_sorter": .string("jev")])
                    note = "Jev now sorts your new mail."
                }
            }
        }
        .confirmationDialog("Remove your TypeSafe key?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove key", role: .destructive) { Task { await removeKey() } }
        } message: {
            Text("Your model sorts your mail again.")
        }
        .alert("Something went wrong", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
        .alert(note ?? "", isPresented: .constant(note != nil)) {
            Button("OK") { note = nil }
        }
    }

    @ViewBuilder
    private func sections(_ o: MailOverview) -> some View {
        Section {
            Toggle(isOn: Binding(get: { o.sorting }, set: { on in Task { await set(["mail_sorting": .bool(on)]) } })) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Sort new mail in the background")
                    Text("Files each new email under what needs a reply, what's important and everything else. Newsletters are recognised without a model.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .disabled(busy)
            .tint(Color.privateTone)
        }

        if o.sorting {
            Section {
                Picker("Sorted by", selection: Binding(get: { o.sorter }, set: { choose($0, o) })) {
                    Text("Your model").tag(MailSorter.model)
                    Text("Jev").tag(MailSorter.jev)
                }
                .pickerStyle(.segmented)
                .disabled(busy)
                .padding(.vertical, 4)
                HStack {
                    Text("Where it runs")
                    Spacer()
                    if o.sorter == .jev {
                        LocalityBadge(locality: .cloud)
                    } else if let l = o.modelLocality ?? o.sorterLocality {
                        LocalityBadge(locality: l)
                    }
                }
            } header: {
                Text("Sorted by")
            } footer: {
                Text(o.sorter == .jev
                     ? "Jev by TypeSafe, in the cloud: the sender, subject and text of new mail are sent to TypeSafe to be sorted (newsletters too when you have smart folders). Suspicious mail never is. No summaries."
                     : "Your model. Or choose Jev, a fast cloud service that only sorts (your own TypeSafe key).")
            }
        }

        if o.jevConnected {
            Section {
                HStack(spacing: 12) {
                    Image(systemName: "lock.fill")
                        .foregroundStyle(Color.cloudTone)
                        .frame(width: 30, height: 30)
                        .background(Color.cloudSoft, in: .rect(cornerRadius: 8))
                    VStack(alignment: .leading, spacing: 2) {
                        Text("TypeSafe key saved")
                        Text("Stored encrypted on your computer, and only sent to TypeSafe.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
                Button("Remove key", role: .destructive) { confirmRemove = true }
            }
        }
    }

    private func choose(_ next: MailSorter, _ o: MailOverview) {
        guard next != o.sorter else { return }
        if next == .jev && !o.jevConnected {
            askingKey = true
            return
        }
        Task { await set(["mail_sorter": .string(next.rawValue)]) }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            overview = try await api.mailOverview()
        } catch is CancellationError {
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func set(_ changes: [String: JSONValue]) async {
        busy = true
        defer { busy = false }
        do {
            try await model.updateSettings(changes)
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func removeKey() async {
        busy = true
        defer { busy = false }
        do {
            try await model.api?.jevDisconnect()
            note = "TypeSafe key removed. Your model sorts your mail again."
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// Asks for the user's TypeSafe key, checked with TypeSafe before it's saved.
private struct JevKeySheet: View {
    let onSaved: () -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var key = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Jev is a cloud service by TypeSafe that only sorts: it answers in a fraction of a second but writes nothing, so there are no summaries. Each new email it sorts or files into smart folders (sender, subject and text) is sent to TypeSafe. Newsletters are only sent when you have smart folders, and suspicious mail never is.")
                        .font(.callout)
                    LocalityBadge(locality: .cloud)
                }
                Section {
                    SecureField("Your TypeSafe API key", text: $key)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } footer: {
                    HStack(spacing: 4) {
                        Text("TypeSafe charges for use.")
                        Button("Get a key") {
                            if let url = URL(string: "https://docs.typesafe.ai/introduction/quickstart") { openURL(url) }
                        }
                        .font(.footnote.weight(.semibold))
                    }
                }
                if let error {
                    Section { Text(error).foregroundStyle(Color.danger) }
                }
            }
            .navigationTitle("Sort mail with Jev")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", systemImage: "xmark") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        Task { await save() }
                    } label: {
                        if busy { ProgressView() } else { Text("Use Jev") }
                    }
                    .disabled(busy || key.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
    }

    private func save() async {
        guard let api = model.api else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            try await api.jevConnect(key.trimmingCharacters(in: .whitespacesAndNewlines))
            key = ""
            onSaved()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
