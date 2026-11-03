import SwiftUI

/// Invitations Mimi can email for an event. Calendars email nobody: after an event with
/// guests is saved, changed or removed, these are offered, and only the user's click
/// sends them, from their own email account.

/// One offer: a button that sends it, or what happened.
struct InvitationRow: View {
    @State var offer: InvitationOffer
    @Environment(AppModel.self) private var model
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        if offer.sentAt != nil {
            Label {
                VStack(alignment: .leading, spacing: 1) {
                    Text(verbatim: ScheduleWords.sentLabel(offer))
                    if let from = offer.from {
                        Text(verbatim: "From \(from)").font(.footnote).foregroundStyle(.secondary)
                    }
                }
            } icon: {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.privateTone)
            }
        } else if offer.from == nil {
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: "Nothing was emailed to \(ScheduleWords.guestNames(offer)).")
                Text("To send invitations from here, connect your email in Mimi on your computer (Settings › Connections).")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        } else {
            VStack(alignment: .leading, spacing: 6) {
                Button {
                    Task { await send() }
                } label: {
                    HStack {
                        if busy { ProgressView().controlSize(.small) } else { Image(systemName: "paperplane.fill") }
                        Text(verbatim: ScheduleWords.offerLabel(offer)).multilineTextAlignment(.leading)
                    }
                    .font(.body.weight(.semibold))
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.roundedRectangle(radius: 12))
                .tint(Color.ink)
                .disabled(busy)
                Text(verbatim: offer.fromNote ?? "From \(offer.from ?? "")."
                ).font(.footnote).foregroundStyle(.secondary)
                if let error {
                    Text(error).font(.footnote).foregroundStyle(Color.danger)
                }
            }
            .padding(.vertical, 4)
        }
    }

    private func send() async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            let sent = try await api.sendInvitations(offer.id)
            withAnimation { offer = sent }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// "Send invitations…" on an event: the invitation for its guests as it is now, shown
/// before anything is emailed. Sending is the user's tap on Send.
struct SendInvitationsSheet: View {
    let event: CalendarEvent
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var offer: InvitationOffer?
    @State private var error: String?
    @State private var busy = false
    private let math = CalendarMath()

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text(verbatim: "For “\(event.title)”, \(math.whenLong(event, now: math.ms(Date()))). Their calendar app lets them answer.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
                }
                if let offer {
                    Section {
                        ForEach(offer.guests, id: \.email) { g in
                            VStack(alignment: .leading, spacing: 1) {
                                Text(verbatim: g.name ?? g.email)
                                if g.name != nil { Text(verbatim: g.email).font(.footnote).foregroundStyle(.secondary) }
                            }
                        }
                    } header: {
                        Text("To")
                    } footer: {
                        Text(verbatim: offer.from == nil
                            ? "To send invitations from here, connect your email in Mimi on your computer (Settings › Connections)."
                            : offer.fromNote ?? "From \(offer.from ?? "").")
                    }
                } else if error == nil {
                    Section {
                        HStack(spacing: 8) {
                            ProgressView().controlSize(.small)
                            Text("Getting them ready…").foregroundStyle(.secondary)
                        }
                    }
                }
                if let error {
                    Section {
                        Label(error, systemImage: "exclamationmark.circle").foregroundStyle(Color.danger)
                    }
                }
            }
            .navigationTitle("Send invitations")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Not now") { dismiss() }
                }
            }
            .safeAreaInset(edge: .bottom) {
                if let offer, offer.from != nil {
                    Button {
                        Task { await send(offer) }
                    } label: {
                        HStack {
                            if busy { ProgressView() } else { Image(systemName: "paperplane.fill") }
                            Text("Send")
                        }
                    }
                    .buttonStyle(.lime)
                    .disabled(busy || offer.sentAt != nil)
                    .padding(.horizontal, 20)
                    .padding(.bottom, 8)
                }
            }
        }
        .presentationDetents([.medium, .large])
        .task { await load() }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            offer = try await api.offerInvitations(event.id)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func send(_ offer: InvitationOffer) async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            let sent = try await api.sendInvitations(offer.id)
            CalendarStore.shared.say(ScheduleWords.sentLabel(sent))
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
