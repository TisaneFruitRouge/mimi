import SwiftUI

/// Settings › This phone: its name and connection, the other phones paired with the
/// computer (managed there), and unpairing.
struct PhoneSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var renaming = false
    @State private var newName = ""
    @State private var confirmUnpair = false
    @State private var error: String?

    private var others: [Device] {
        (model.remote?.devices ?? []).filter { $0.id != model.paired?.deviceId }
    }

    var body: some View {
        List {
            Section {
                Button {
                    newName = model.paired?.deviceName ?? ""
                    renaming = true
                } label: {
                    LabeledContent("Name") {
                        HStack(spacing: 6) {
                            Text(model.paired?.deviceName ?? "")
                            Image(systemName: "pencil").font(.footnote)
                        }
                    }
                }
                .tint(.primary)
                LabeledContent("Connection") {
                    HStack(spacing: 6) {
                        Circle().fill(dotColor).frame(width: 7, height: 7)
                        Text(connection)
                    }
                    .foregroundStyle(.secondary)
                }
            } header: {
                Text("This phone")
            } footer: {
                Text("This is how your computer lists this phone.")
            }

            Section("How it connects") {
                ExplainedRow(systemImage: "checkmark.shield.fill", tint: .privateSoft, foreground: .privateTone,
                             title: "Encrypted from end to end",
                             detail: "This phone checks it's talking to your computer, and only phones you paired get in.")
                ExplainedRow(systemImage: "wifi", tint: .networkSoft, foreground: .networkTone,
                             title: "Straight to your computer when it can",
                             detail: "On the same Wi-Fi, and on most other networks, this phone connects directly.")
                ExplainedRow(systemImage: "globe", tint: .networkSoft, foreground: .networkTone,
                             title: model.paired?.relay != nil ? "Through your own relay otherwise" : "Through a public relay otherwise",
                             detail: model.paired?.relay != nil
                                ? "When a direct connection isn't possible, your relay passes the encrypted messages along."
                                : "When a direct connection isn't possible, a relay run by iroh's makers passes the encrypted messages along. It can't read them, but it can see that this phone and your computer are talking.")
            }

            Section {
                if others.isEmpty {
                    Text("No other phone is paired.").foregroundStyle(.secondary)
                }
                ForEach(others) { d in
                    ExplainedRow(systemImage: "iphone", tint: Color(hex: 0x30D158), title: d.name, detail: deviceDetail(d))
                }
            } header: {
                Text("Other phones")
            } footer: {
                Text("Pair, rename or remove phones on your computer, in Settings › Phone.")
            }

            Section {
                Button("Unpair this phone", role: .destructive) { confirmUnpair = true }
            } footer: {
                Text("This phone forgets your computer, and your computer forgets this phone. Your conversations stay on your computer.")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("This phone")
        .navigationBarTitleDisplayMode(.inline)
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
        .problemAlert($error)
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

    private var dotColor: Color {
        switch model.phase {
        case .online: .privateTone
        case .connecting: .cloudTone
        default: .danger
        }
    }

    private func deviceDetail(_ d: Device) -> String {
        if d.connection == .direct { return "Connected now" }
        if d.connection == .relayed { return "Connected now, through the relay" }
        if let seen = d.lastSeenAt { return "Last connected \(timeSince(seen))" }
        return "Paired"
    }
}
