import SwiftUI

@main
struct MimiApp: App {
    @State private var model = AppModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .tint(Color.ink)
                .onOpenURL { url in
                    if let link = PairingLink(url) { model.incomingLink = link }
                }
                .task { await model.refresh() }
                .onChange(of: scenePhase) { old, new in
                    switch new {
                    case .active where old == .background: Task { await model.resume() }
                    case .background: model.pause()
                    default: break
                    }
                }
        }
    }
}

/// Pairing until there's a computer, then the panels.
struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            switch model.phase {
            case .unpaired(let reason):
                WelcomeView(reason: reason)
                    .transition(.opacity)
            default:
                MainTabs()
                    .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.25), value: model.paired == nil)
    }
}

struct MainTabs: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        TabView(selection: $model.tab) {
            Tab("Chats", systemImage: "bubble.left.and.bubble.right", value: AppTab.chats) {
                ConversationsView()
            }
            Tab("Calendar", systemImage: "calendar", value: AppTab.calendar) { CalendarView() }
            Tab("Mail", systemImage: "envelope", value: AppTab.mail) { MailView() }
            Tab("People", systemImage: "person.2", value: AppTab.people) { PeopleView() }
            Tab("Settings", systemImage: "gearshape", value: AppTab.settings) {
                SettingsView()
            }
        }
        .overlay(alignment: .top) {
            ConnectionBanner()
        }
    }
}

/// Says when the computer can't be reached, with the sleepy mochi and a retry.
private struct ConnectionBanner: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            if model.phase == .offline {
                HStack(spacing: 10) {
                    AssistantAvatar(size: 30, mood: .sleepy)
                    VStack(alignment: .leading, spacing: 1) {
                        Text("Can't reach your computer").font(.subheadline.weight(.semibold))
                        Text("Is it on, awake and connected?").font(.footnote).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    Button("Retry") { model.retryNow() }
                        .font(.subheadline.weight(.semibold))
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .glassEffect(.regular, in: .rect(cornerRadius: 20))
                .padding(.horizontal, 12)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: model.phase)
    }
}
