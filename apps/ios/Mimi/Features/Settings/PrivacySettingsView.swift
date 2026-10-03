import SwiftUI

/// Settings › Privacy: what stays on the computer, and what goes elsewhere when asked.
struct PrivacySettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var connections: [ConnectionItem] = []

    var body: some View {
        List {
            Section {
                Text("What stays on your computer, and what goes elsewhere when you ask for it.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
            }

            Section("Your data") {
                ExplainedRow(systemImage: "lock.fill", tint: .privateSoft, foreground: .privateTone,
                             title: "Stored on your computer",
                             detail: "Conversations, memory, settings and passwords are encrypted there, and nowhere else.")
                ExplainedRow(systemImage: "checkmark.shield.fill", tint: .privateSoft, foreground: .privateTone,
                             title: "No account, no tracking",
                             detail: "Mimi has no server of its own. Nothing is sent to the people who make it.")
                ExplainedRow(systemImage: "iphone", tint: .privateSoft, foreground: .privateTone,
                             title: "This phone keeps only its key",
                             detail: "What it needs to reach your computer is in this phone's Keychain. Your conversations and memory stay on your computer.")
                NavigationLink {
                    MemorySettingsView()
                } label: {
                    ExplainedRow(systemImage: "book.fill", tint: Color(hex: 0xBF5AF2),
                                 title: "What \(model.assistantName) remembers", detail: "See, change or forget it")
                }
            }

            if !model.providers.isEmpty {
                Section {
                    ForEach(model.providers) { p in
                        HStack {
                            Text(p.name)
                            Spacer()
                            LocalityBadge(locality: p.locality)
                        }
                    }
                    NavigationLink("Change model") { ModelsSettingsView() }
                } header: {
                    Text("Where your messages go")
                } footer: {
                    Text("Your messages go only to the model source you use. Cloud services receive them; models on your computer or your network keep them at home.")
                }
            }

            // Privacy: the mail sorting section (Mail) goes here.

            Section("Phone") {
                NavigationLink {
                    PhoneSettingsView()
                } label: {
                    ExplainedRow(systemImage: "iphone", tint: .networkSoft, foreground: .networkTone,
                                 title: phoneCount == 1 ? "1 phone can reach your computer" : "\(phoneCount) phones can reach your computer",
                                 detail: "End-to-end encrypted. When they can't connect directly, a relay passes the messages along without being able to read them.")
                }
            }

            if connections.contains(where: { $0.integration == "telegram" }) {
                Section("Telegram") {
                    ExplainedRow(systemImage: "paperplane.fill", tint: .networkSoft, foreground: .networkTone,
                                 title: "Bot messages aren't end-to-end encrypted",
                                 detail: "Telegram can read what you and your bot send each other. Keep private things in this app.")
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Privacy")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: model.revision("connections_changed")) {
            if let list = try? await model.api?.connections() { connections = list }
        }
    }

    private var phoneCount: Int { max(model.remote?.devices.count ?? 1, 1) }
}
