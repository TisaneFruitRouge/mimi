import SwiftUI

/// Conversations, newest first, with search and a new-chat button.
struct ConversationsView: View {
    @Environment(AppModel.self) private var model
    @State private var path: [UUID] = []
    @State private var search = ""
    @State private var deleting: Conversation?
    @State private var error: String?

    private var shown: [Conversation] {
        let q = search.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return model.conversations }
        return model.conversations.filter { $0.title.localizedCaseInsensitiveContains(q) }
    }

    var body: some View {
        NavigationStack(path: $path) {
            List {
                ForEach(shown) { c in
                    NavigationLink(value: c.id) {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(c.title.isEmpty ? "New chat" : c.title)
                                .font(.body.weight(.medium))
                                .lineLimit(1)
                            Text(relativeDay(c.updatedAt))
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                        }
                        .padding(.vertical, 2)
                    }
                    .swipeActions {
                        Button("Delete", systemImage: "trash", role: .destructive) { deleting = c }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .overlay {
                if model.conversations.isEmpty && model.phase == .online {
                    ContentUnavailableView {
                        Label { Text("No conversations yet") } icon: { AssistantAvatar(size: 72, mood: .idle) }
                    } description: {
                        Text("Start one with the pencil button.")
                    }
                } else if !search.isEmpty && shown.isEmpty {
                    ContentUnavailableView.search(text: search)
                }
            }
            .searchable(text: $search, prompt: "Search conversations")
            .navigationTitle("Chats")
            .navigationDestination(for: UUID.self) { ChatView(conversationId: $0) }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("New chat", systemImage: "square.and.pencil") { newChat() }
                        .disabled(model.phase != .online)
                }
            }
            .refreshable { await model.refresh() }
            .confirmationDialog(
                "Delete “\(deleting?.title ?? "")”?",
                isPresented: .constant(deleting != nil),
                titleVisibility: .visible
            ) {
                Button("Delete", role: .destructive) {
                    guard let c = deleting else { return }
                    deleting = nil
                    Task {
                        do { try await model.delete(c.id) } catch { self.error = error.localizedDescription }
                    }
                }
                Button("Cancel", role: .cancel) { deleting = nil }
            } message: {
                Text("It's deleted on your computer too.")
            }
            .alert("Something went wrong", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .mimiNewChat)) { _ in newChat() }
        // "Ask Mimi about this" from another panel: a new chat, which takes the draft.
        .onChange(of: model.askDraft) { _, draft in
            if draft != nil { newChat() }
        }
        // A routine's result opened from the banner or Calendar.
        .onChange(of: model.openConversation) { _, id in
            guard let id else { return }
            model.openConversation = nil
            path = [id]
        }
    }

    private func newChat() {
        Task {
            do {
                let c = try await model.newConversation()
                path = [c.id]
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}

extension Notification.Name {
    /// Opens a new chat (the app's shortcut, or a quick action).
    static let mimiNewChat = Notification.Name("mimiNewChat")
}
