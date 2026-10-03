import SwiftUI

/// Settings: this phone and its connection, then the assistant's own settings (more pages
/// join as they come to the phone).
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var renaming = false
    @State private var newName = ""
    @State private var confirmUnpair = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Name") {
                        Button(model.paired?.deviceName ?? "") {
                            newName = model.paired?.deviceName ?? ""
                            renaming = true
                        }
                    }
                    LabeledContent("Connection") {
                        Text(connection).foregroundStyle(.secondary)
                    }
                } header: {
                    Text("This phone")
                } footer: {
                    Text(footer)
                }

                if let locality = model.modelLocality {
                    Section("Your assistant") {
                        LabeledContent("Name", value: model.assistantName)
                        LabeledContent("Model") { LocalityBadge(locality: locality) }
                    }
                }

                Section {
                    NavigationLink("Reminders & notifications") { NotificationsSettingsView() }
                }

                Section {
                    Button("Unpair this phone", role: .destructive) { confirmUnpair = true }
                } footer: {
                    Text("This phone forgets your computer, and your computer forgets this phone. Your conversations stay on your computer.")
                }
            }
            .navigationTitle("Settings")
            .alert("Rename this phone", isPresented: $renaming) {
                TextField("Name", text: $newName)
                Button("Cancel", role: .cancel) {}
                Button("Save") {
                    let name = newName.trimmingCharacters(in: .whitespaces)
                    guard !name.isEmpty else { return }
                    Task {
                        do { try await model.renameThisPhone(name) } catch { self.error = error.localizedDescription }
                    }
                }
            }
            .confirmationDialog("Unpair this phone?", isPresented: $confirmUnpair, titleVisibility: .visible) {
                Button("Unpair", role: .destructive) { Task { await model.unpair() } }
            } message: {
                Text("To use \(model.assistantName) here again, you'll scan a new code on your computer.")
            }
            .alert("Something went wrong", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
    }

    private var connection: String {
        switch model.phase {
        case .online:
            switch model.path {
            case .direct: "Direct"
            case .relayed: "Through the relay"
            case nil: "Connected"
            }
        case .connecting: "Connecting…"
        case .offline: "Can't reach your computer"
        case .unpaired: "Not paired"
        }
    }

    private var footer: String {
        if model.paired?.relay != nil {
            return "End-to-end encrypted. When your phone can't reach your computer directly, your own relay passes the messages along."
        }
        return "End-to-end encrypted. When your phone can't reach your computer directly, a public relay passes the messages along without being able to read them."
    }
}
