import SwiftUI

/// Where the People tab can go.
enum PeopleRoute: Hashable {
    case person(UUID)
    case duplicates
    case removed
    case conversation(UUID)
    case memoryNote(String)
}

/// Who a merge starts from: the person kept, and who is folded into them.
struct MergePlan: Identifiable, Equatable {
    var keep: UUID
    var others: [UUID]
    var id: String { ([keep] + others).map(\.uuidString).joined(separator: ",") }
}

/// The People tab's navigation and the sheets shared by the list and a person's page:
/// merging, deleting, and the notices that offer Undo.
@Observable
final class PeopleFlow {
    var path: [PeopleRoute] = []
    /// "Merge with…": choosing who else is this person.
    var pickingFor: PersonSummary?
    /// The confirmation before merging.
    var plan: MergePlan?
    /// The confirmation before deleting.
    var deleting: PersonSummary?
    var notice: UndoNotice?
    var error: String?

    /// Shows someone, replacing whatever was open.
    func open(_ id: UUID) { path = [.person(id)] }

    /// An old id of someone merged into another person: show them under their id now.
    func moved(from old: UUID, to new: UUID) {
        if let i = path.lastIndex(of: .person(old)) { path[i] = .person(new) }
    }

    /// Leaves a person's page after they were deleted or merged away.
    func close(_ id: UUID) {
        path.removeAll { $0 == .person(id) }
    }

    /// Says a merge happened, with an exact Undo (everyone back under their own id).
    func merged(_ done: MergeResult, api: MimiAPI?) {
        open(done.person.id)
        notice = UndoNotice(text: "\(done.person.name) is one contact now") { [weak self] in
            guard let api else { return }
            let back = try await api.undoMerge(done.mergeId)
            self?.open(back.id)
            self?.notice = UndoNotice(text: "Separated again")
        }
    }
}

/// Attaches the flow's sheets and alerts to the People tab.
struct PeopleFlowSheets: ViewModifier {
    @Bindable var flow: PeopleFlow
    @Environment(AppModel.self) private var model
    @State private var doomed: Person?

    func body(content: Content) -> some View {
        content
            .sheet(item: $flow.pickingFor) { person in
                MergePicker(person: person) { other in
                    flow.pickingFor = nil
                    flow.plan = MergePlan(keep: person.id, others: [other])
                }
            }
            .sheet(item: $flow.plan) { plan in
                MergeSheet(plan: plan) { done in
                    flow.plan = nil
                    for id in plan.others { flow.close(id) }
                    flow.merged(done, api: model.api)
                }
            }
            .task(id: flow.deleting?.id) {
                doomed = nil
                guard let id = flow.deleting?.id else { return }
                doomed = try? await model.api?.person(id)
            }
            .alert(
                "Delete \(flow.deleting?.name ?? "")?",
                isPresented: Binding(get: { flow.deleting != nil }, set: { if !$0 { flow.deleting = nil } })
            ) {
                Button("Cancel", role: .cancel) { flow.deleting = nil }
                Button("Delete", role: .destructive) {
                    guard let p = flow.deleting else { return }
                    flow.deleting = nil
                    Task { await delete(p) }
                }
            } message: {
                Text(consequences)
            }
            .undoNotice($flow.notice)
            .problemAlert($flow.error)
    }

    /// What deleting someone does, in plain words.
    private var consequences: String {
        let name = flow.deleting?.name ?? ""
        let kept = "What \(model.assistantName) remembers about them is kept."
        // Only cards from address books can be brought back; what was added by hand can't.
        if let p = doomed, !p.hasImportedCards {
            return "\(name) and the ways to reach them you added will be deleted. \(kept) This can't be undone."
        }
        let own = doomed?.handles.contains { $0.isOwn } == true
            ? " The numbers and addresses you added here are deleted." : ""
        return "\(name) will be removed from Mimi only. Your address books and email aren't changed, "
            + "and \(name) won't reappear when they refresh.\(own) \(kept) "
            + "You can bring them back later from Removed contacts."
    }

    private func delete(_ p: PersonSummary) async {
        guard let api = model.api else { return }
        do {
            let removed = try await api.removePerson(p.id)
            flow.close(p.id)
            if let removed {
                flow.notice = UndoNotice(text: "\(p.name) was removed from Mimi") { [flow] in
                    let back = try await api.restorePerson(removed.id)
                    flow.open(back.id)
                    flow.notice = UndoNotice(text: "\(back.name) is back")
                }
            } else {
                flow.notice = UndoNotice(text: "\(p.name) was deleted")
            }
        } catch {
            flow.error = error.localizedDescription
        }
    }
}

extension Person {
    var summary: PersonSummary {
        var channels: [Channel] = []
        for h in handles where !channels.contains(h.channel) { channels.append(h.channel) }
        return PersonSummary(id: id, name: name, nickname: nickname, channels: channels, reach: handles.prefix(3).map(\.value))
    }
}
