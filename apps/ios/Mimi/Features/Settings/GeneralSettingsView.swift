import SwiftUI

extension AppModel {
    /// A setting the app's `Settings` doesn't decode, read from the computer's own copy.
    func setting(_ key: String) -> JSONValue? { settingsJSON[key] }

    /// A switch bound to a yes/no setting; failures go to `error`.
    func settingToggle(_ key: String, default fallback: Bool = false, error problem: Binding<String?>) -> Binding<Bool> {
        Binding {
            self.setting(key)?.bool ?? fallback
        } set: { on in
            Task {
                do { try await self.updateSettings([key: .bool(on)]) } catch { problem.wrappedValue = error.localizedDescription }
            }
        }
    }
}

/// Settings › General: the assistant, updates, and what only the computer can change.
struct GeneralSettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var updates: UpdateStatus?
    @State private var checking = false
    @State private var confirmWelcome = false
    @State private var welcomeSent = false
    @State private var error: String?

    var body: some View {
        List {
            Section("Assistant") {
                NavigationLink {
                    PersonalitySettingsView()
                } label: {
                    ExplainedRow(systemImage: "face.smiling.inverse", tint: Color(hex: 0xFF2D55),
                                 title: "Name and personality",
                                 detail: "\(model.assistantName), how it talks and what it keeps in mind")
                }
                Button {
                    confirmWelcome = true
                } label: {
                    ExplainedRow(systemImage: "arrow.counterclockwise", tint: Color(.tertiarySystemFill), foreground: .secondary,
                                 title: "Show the welcome again",
                                 detail: welcomeSent
                                    ? "It's waiting on your computer. Open Mimi there to go through it."
                                    : "On your computer: choose a model, connect apps and introduce yourself.")
                }
                .tint(.primary)
            }

            Section {
                ExplainedRow(systemImage: "arrow.triangle.2.circlepath", tint: .networkSoft, foreground: .networkTone,
                             title: "Check for new versions",
                             detail: "Once a day, your computer asks GitHub, where Mimi is published, whether there's a newer version. Nothing about you is sent.") {
                    Toggle("Check for new versions", isOn: model.settingToggle("update_check", error: $error))
                        .labelsHidden()
                }
                ExplainedRow(systemImage: "arrow.down.circle.fill",
                             tint: updates?.available != nil ? .limeSoft : Color(.tertiarySystemFill),
                             foreground: updates?.available != nil ? .limeDeep : .secondary,
                             title: updates?.available != nil ? "A new version is available" : "Version",
                             detail: versionDetail)
                if let available = updates?.available, let url = URL(string: available.url) {
                    Button("See what's new") { openURL(url) }
                }
                Button {
                    Task { await checkNow() }
                } label: {
                    HStack {
                        Text("Check now")
                        if checking { Spacer(); ProgressView() }
                    }
                }
                .disabled(checking)
            } header: {
                Text("Updates")
            } footer: {
                Text("Updates are installed on your computer. Mimi on this phone updates through the App Store or Xcode.")
            }

            Section {
                ExplainedRow(systemImage: "power", tint: .privateSoft, foreground: .privateTone,
                             title: "Keep Mimi running in the background",
                             detail: "Lets Telegram and reminders work with Mimi's window closed. Turn it on or off on your computer, in Settings › General.")
            } header: {
                Text("On your computer")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("General")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: model.revision("update_changed")) {
            if let status = try? await model.api?.updates() { updates = status }
        }
        .confirmationDialog("Show the welcome again?", isPresented: $confirmWelcome, titleVisibility: .visible) {
            Button("Show it on my computer") { Task { await showWelcome() } }
        } message: {
            Text("Mimi on your computer opens the welcome, where you can choose a model, connect apps and introduce yourself. Nothing is lost.")
        }
        .problemAlert($error)
    }

    private var versionDetail: String {
        guard let s = updates else { return "Checking…" }
        if let available = s.available { return "Version \(available.version) is out. Your computer has \(s.current)." }
        if let error = s.error { return error }
        if let checked = s.checkedAt { return "Your computer has the latest version (\(s.current)). Checked \(timeSince(checked))." }
        return "Your computer has version \(s.current)."
    }

    private func checkNow() async {
        checking = true
        defer { checking = false }
        do {
            updates = try await model.api?.checkForUpdates()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func showWelcome() async {
        do {
            try await model.updateSettings(["onboarding_done": .bool(false), "onboarding_step": .number(0)])
            welcomeSent = true
        } catch {
            self.error = error.localizedDescription
        }
    }
}
