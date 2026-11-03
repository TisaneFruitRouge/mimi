import SwiftUI

/// Settings, like System Settings: the assistant on top, then one row per page. The pages
/// are the desktop's (General, Personality, Connections, Models, Memory, Reminders &
/// notifications, Permissions, Privacy) plus this phone's.
///
/// Adding a page is one line where it belongs:
/// `SettingsLink("Title", "sf.symbol", tint) { YourPage() }`.
struct SettingsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        NavigationStack {
            List {
                Section {
                    NavigationLink {
                        PersonalitySettingsView()
                    } label: {
                        AssistantCard()
                    }
                }

                Section {
                    SettingsLink("General", "gearshape.fill", Color(hex: 0x8E8E93)) { GeneralSettingsView() }
                    SettingsLink("Personality", "face.smiling.inverse", Color(hex: 0xFF2D55)) { PersonalitySettingsView() }
                }

                Section {
                    SettingsLink("Connections", "square.grid.2x2.fill", Color(hex: 0x0A84FF)) { ConnectionsSettingsView() }
                    SettingsLink("Models", "sparkles", .lime, foreground: .limeInk) { ModelsSettingsView() }
                    SettingsLink("Memory", "book.fill", Color(hex: 0xBF5AF2)) { MemorySettingsView() }
                    SettingsLink("Reminders & notifications", "bell.badge.fill", Color(hex: 0xFF3B30)) { NotificationsSettingsView() }
                }

                Section {
                    SettingsLink("Permissions", "hand.raised.fill", Color(hex: 0xFF9F0A)) { PermissionsSettingsView() }
                    SettingsLink("Privacy", "lock.fill", .privateTone) { PrivacySettingsView() }
                }

                Section {
                    SettingsLink("This phone", "iphone", Color(hex: 0x30D158), value: model.paired?.deviceName) {
                        PhoneSettingsView()
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Settings")
            .refreshable { await model.refresh() }
        }
    }
}

/// The assistant at the top of Settings: its face, its name, and where its model runs.
private struct AssistantCard: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 14) {
            AssistantAvatar(size: 56, mood: .idle)
            VStack(alignment: .leading, spacing: 4) {
                Text(model.assistantName)
                    .font(.title3.weight(.semibold))
                if let locality = model.modelLocality {
                    LocalityBadge(locality: locality)
                } else {
                    Text("Name, personality and what it keeps in mind")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 4)
    }
}
