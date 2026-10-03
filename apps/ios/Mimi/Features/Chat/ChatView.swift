import SwiftUI

/// One conversation: the thread, and the composer floating over it.
struct ChatView: View {
    let conversationId: UUID
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var renaming = false
    @State private var newTitle = ""
    @State private var confirmDelete = false
    @State private var error: String?

    private var detail: ConversationDetail? { model.details[conversationId] }
    private var messages: [Message] { detail?.messages ?? [] }
    private var replying: Bool { messages.last?.status == .streaming }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 22) {
                    if detail != nil && messages.isEmpty {
                        EmptyChat()
                    }
                    ForEach(messages) { message in
                        MessageView(message: message)
                            .id(message.id)
                            .transition(.asymmetric(insertion: .move(edge: .bottom).combined(with: .opacity), removal: .opacity))
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.horizontal, 18)
                .padding(.top, 12)
                .animation(.spring(response: 0.4, dampingFraction: 0.85), value: messages.count)
            }
            .scrollDismissesKeyboard(.interactively)
            .defaultScrollAnchor(.bottom)
            .onChange(of: messages.last?.content.count) { _, _ in
                proxy.scrollTo("bottom", anchor: .bottom)
            }
            .onChange(of: messages.count) { _, _ in
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .safeAreaInset(edge: .bottom) {
                Composer(replying: replying) { text, mentions in
                    try await model.send(text, mentions: mentions, in: conversationId)
                    withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
                } onStop: {
                    try? await model.api?.cancel(conversationId)
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 8)
            }
        }
        .background(Color.canvas)
        .navigationTitle(detail?.conversation.title.isEmpty == false ? detail!.conversation.title : "New chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Rename", systemImage: "pencil") {
                        newTitle = detail?.conversation.title ?? ""
                        renaming = true
                    }
                    Button("Delete", systemImage: "trash", role: .destructive) { confirmDelete = true }
                } label: {
                    Image(systemName: "ellipsis")
                }
                .accessibilityLabel("Conversation options")
            }
        }
        .task { await model.loadConversation(conversationId) }
        .alert("Rename conversation", isPresented: $renaming) {
            TextField("Name", text: $newTitle)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                let title = newTitle.trimmingCharacters(in: .whitespaces)
                guard !title.isEmpty else { return }
                Task {
                    do { try await model.rename(conversationId, to: title) } catch { self.error = error.localizedDescription }
                }
            }
        }
        .alert("Delete this conversation?", isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) {
                Task {
                    do {
                        try await model.delete(conversationId)
                        dismiss()
                    } catch {
                        self.error = error.localizedDescription
                    }
                }
            }
        } message: {
            Text("It's deleted on your computer too. What your assistant learned from it stays in its memory.")
        }
        .alert("Something went wrong", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }
}

private struct EmptyChat: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(spacing: 14) {
            AssistantAvatar(size: 88, mood: .idle)
            Text("What can I do for you?")
                .font(.title3.weight(.semibold))
            Text("Ask \(model.assistantName) anything, or tag someone with @ and an email with #.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 80)
    }
}
