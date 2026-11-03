import SwiftUI

/// Connecting an account from the phone. Everything is checked against the real service
/// by the computer before it's saved, so a saved connection works.
struct ConnectFlowView: View {
    let integration: String
    let onDone: () -> Void

    var body: some View {
        switch integration {
        case "google_calendar": GoogleCalendarConnect(onDone: onDone)
        case "caldav", "carddav": CalDAVConnect(onDone: onDone)
        case "telegram": TelegramConnect(onDone: onDone)
        case "email": EmailConnect(onDone: onDone)
        default:
            ContentUnavailableView("Not on the phone yet", systemImage: "desktopcomputer",
                                   description: Text("Connect this on your computer, in Settings › Connections."))
        }
    }
}

/// Submits a setup and keeps the error to show inline.
@Observable
private final class ConnectAttempt {
    var busy = false
    var error: String?
    var created: ConnectionItem?

    func connect(_ setup: ConnectionSetup, api: MimiAPI?) async {
        guard let api else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            created = try await api.connect(setup)
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// "Connected": a big check, the connection's own line, and Done.
private struct ConnectedView: View {
    let connection: ConnectionItem
    let onDone: () -> Void
    var another: (() -> Void)?

    var body: some View {
        VStack(spacing: 18) {
            Spacer()
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 64))
                .foregroundStyle(Color.privateTone)
                .symbolEffect(.bounce, value: connection.id)
            VStack(spacing: 4) {
                Text("\(connection.name) is connected").font(.title3.weight(.semibold)).multilineTextAlignment(.center)
                Text(connection.detail).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            Spacer()
            Button("Done", action: onDone).buttonStyle(SettingsInkButtonStyle())
            if let another {
                Button("Add another calendar", action: another)
            }
        }
        .padding(24)
        .navigationBarBackButtonHidden()
    }
}

/// A calm note at the bottom of a form: a lock for privacy, a warning otherwise.
private struct FormNote: View {
    var warning = false
    let text: String

    var body: some View {
        Label {
            Text(text).font(.footnote)
        } icon: {
            Image(systemName: warning ? "exclamationmark.triangle.fill" : "lock.fill")
        }
        .foregroundStyle(warning ? Color.cloudTone : Color.privateTone)
        .listRowBackground(warning ? Color.cloudSoft : Color.privateSoft)
    }
}

/// A numbered step in a setup.
private struct StepRow<Content: View>: View {
    let n: Int
    @ViewBuilder let content: () -> Content

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text("\(n)")
                .font(.footnote.weight(.semibold))
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 24)
                .background(Color(.tertiarySystemFill), in: .circle)
            VStack(alignment: .leading, spacing: 8) { content() }
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 2)
    }
}

// MARK: Google Calendar

private struct GoogleCalendarConnect: View {
    let onDone: () -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var signInAvailable: Bool?
    @State private var url = ""
    @State private var attempt = ConnectAttempt()

    var body: some View {
        Group {
            if let created = attempt.created {
                ConnectedView(connection: created, onDone: onDone) {
                    url = ""
                    attempt.created = nil
                }
            } else {
                Form {
                    if signInAvailable == true {
                        Section {
                            Label {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text("Sign in with Google on your computer").font(.body.weight(.medium))
                                    Text("Google sends its answer straight back to your computer, so signing in happens there: open Mimi on your computer, then Settings › Connections › Google Calendar. Your events then go only between your computer and Google.")
                                        .font(.footnote).foregroundStyle(.secondary)
                                }
                            } icon: {
                                Image(systemName: "desktopcomputer").foregroundStyle(Color.networkTone)
                            }
                        } footer: {
                            Text("Or, without signing in, read a calendar through its private address:")
                        }
                    } else if signInAvailable == false {
                        Section {
                        } footer: {
                            Text("\(model.assistantName) reads the calendar through its private address.")
                        }
                    }
                    Section("Private address") {
                        StepRow(n: 1) {
                            Text("Open your Google Calendar settings. They're easiest to find on a computer.")
                            Button("Open Google Calendar settings") {
                                if let u = URL(string: "https://calendar.google.com/calendar/r/settings") { openURL(u) }
                            }
                            .font(.subheadline.weight(.medium))
                        }
                        StepRow(n: 2) {
                            Text("Under **Settings for my calendars**, pick a calendar and choose **Integrate calendar**.")
                        }
                        StepRow(n: 3) {
                            Text("Copy the **Secret address in iCal format** and paste it here.")
                            HStack {
                                TextField("https://calendar.google.com/…/basic.ics", text: $url)
                                    .textInputAutocapitalization(.never)
                                    .autocorrectionDisabled()
                                    .keyboardType(.URL)
                                PasteButton(payloadType: String.self) { strings in
                                    if let s = strings.first { url = s.trimmingCharacters(in: .whitespacesAndNewlines) }
                                }
                                .labelStyle(.iconOnly)
                                .buttonBorderShape(.circle)
                            }
                        }
                        if let e = attempt.error { Text(e).font(.footnote).foregroundStyle(Color.danger) }
                    }
                    Section {
                        FormNote(text: "Anyone with this address can read the calendar, so it's stored encrypted on your computer. To add events, \(model.assistantName) prepares them in Google Calendar and you press Save.")
                    }
                }
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        if attempt.busy { ProgressView() } else {
                            Button("Connect") { Task { await attempt.connect(.googleCalendar(icsUrl: url), api: model.api) } }
                                .disabled(url.trimmingCharacters(in: .whitespaces).isEmpty)
                        }
                    }
                }
            }
        }
        .navigationTitle("Google Calendar")
        .navigationBarTitleDisplayMode(.inline)
        .task { signInAvailable = (try? await model.api?.googleSignInInfo())?.available ?? false }
    }
}

// MARK: CalDAV

private struct CalDAVConnect: View {
    let onDone: () -> Void

    private struct Service: Identifiable, Hashable {
        var id: String
        var name: String
        var server: String
        var user: String
        var help: String
        var helpUrl: String?
    }

    private static let services = [
        Service(id: "icloud", name: "iCloud", server: "https://caldav.icloud.com", user: "Apple Account email",
                help: "Create an app-specific password in your Apple Account, under Sign-In and Security.",
                helpUrl: "https://account.apple.com/account/manage"),
        Service(id: "fastmail", name: "Fastmail", server: "https://caldav.fastmail.com", user: "Fastmail email",
                help: "Create an app password in Fastmail's settings, under Privacy & Security, with calendar access.",
                helpUrl: "https://app.fastmail.com/settings/security/apppasswords"),
        Service(id: "nextcloud", name: "Nextcloud", server: "", user: "Nextcloud username",
                help: "Create an app password in Nextcloud, under Settings › Security.", helpUrl: nil),
        Service(id: "other", name: "Other", server: "", user: "Username",
                help: "Use the address, username and password your calendar service gives for CalDAV.", helpUrl: nil),
    ]

    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var serviceId = "icloud"
    @State private var server = ""
    @State private var username = ""
    @State private var password = ""
    @State private var attempt = ConnectAttempt()

    private var service: Service { Self.services.first { $0.id == serviceId }! }
    private var serverUrl: String { service.server.isEmpty ? server : service.server }
    private var ready: Bool {
        !serverUrl.trimmingCharacters(in: .whitespaces).isEmpty && !username.trimmingCharacters(in: .whitespaces).isEmpty && !password.isEmpty
    }

    var body: some View {
        Group {
            if let created = attempt.created {
                ConnectedView(connection: created, onDone: onDone)
            } else {
                Form {
                    Section {
                        Picker("Service", selection: $serviceId) {
                            ForEach(Self.services) { Text($0.name).tag($0.id) }
                        }
                        .pickerStyle(.segmented)
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets())
                    } footer: {
                        Text("Read, add and change events in iCloud, Fastmail, Nextcloud and other calendars. Address books come with the same account.")
                    }
                    Section {
                        if service.server.isEmpty {
                            TextField(serviceId == "nextcloud" ? "https://cloud.example.com" : "Server address", text: $server)
                                .keyboardType(.URL)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                        }
                        TextField(service.user, text: $username)
                            .textContentType(.username)
                            .keyboardType(.emailAddress)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        SecureField("App password", text: $password)
                    } footer: {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(service.help)
                            if let help = service.helpUrl, let u = URL(string: help) {
                                Button("Open \(service.name) settings") { openURL(u) }
                                    .font(.footnote.weight(.semibold))
                            }
                            if let e = attempt.error { Text(e).foregroundStyle(Color.danger) }
                        }
                    }
                    Section {
                        FormNote(text: "Your password is stored encrypted on your computer and only ever sent to \(serviceId == "other" ? "your calendar server" : service.name).")
                    }
                }
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        if attempt.busy { ProgressView() } else {
                            Button("Connect") {
                                Task { await attempt.connect(.caldav(serverUrl: serverUrl, username: username, password: password), api: model.api) }
                            }
                            .disabled(!ready)
                        }
                    }
                }
            }
        }
        .navigationTitle("Calendar account")
        .navigationBarTitleDisplayMode(.inline)
    }
}

// MARK: Telegram

private struct TelegramConnect: View {
    let onDone: () -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var token = ""
    @State private var attempt = ConnectAttempt()
    /// The connection as it is now: it turns OK once the owner presses Start.
    @State private var live: ConnectionItem?

    var body: some View {
        Group {
            if let live, live.status == .ok {
                ConnectedView(connection: live, onDone: onDone)
            } else if let live {
                pairing(live)
            } else {
                form
            }
        }
        .navigationTitle("Telegram")
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: attempt.created) { _, c in live = c }
        .task(id: model.revision("connections_changed")) {
            guard let id = live?.id, let list = try? await model.api?.connections() else { return }
            if let fresh = list.first(where: { $0.id == id }) { live = fresh }
        }
    }

    private var form: some View {
        Form {
            Section {
            } footer: {
                Text("Chat with \(model.assistantName) from Telegram, through a bot only you can use.")
            }
            Section {
                StepRow(n: 1) {
                    Text("Open **@BotFather**, Telegram's official bot for creating bots.")
                    Button("Open @BotFather") {
                        if let u = URL(string: "https://t.me/BotFather") { openURL(u) }
                    }
                    .font(.subheadline.weight(.medium))
                }
                StepRow(n: 2) {
                    Text("Send \(Text("/newbot").font(.system(.body, design: .monospaced))), then choose a name and a username ending in “bot”.")
                }
                StepRow(n: 3) {
                    Text("Copy the token BotFather gives you, and paste it here.")
                    HStack {
                        SecureField("123456789:AA…", text: $token)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        PasteButton(payloadType: String.self) { strings in
                            if let s = strings.first { token = s.trimmingCharacters(in: .whitespacesAndNewlines) }
                        }
                        .labelStyle(.iconOnly)
                        .buttonBorderShape(.circle)
                    }
                }
                if let e = attempt.error { Text(e).font(.footnote).foregroundStyle(Color.danger) }
            }
            Section {
                FormNote(warning: true, text: "Telegram messages aren't end-to-end encrypted: Telegram can read what you and \(model.assistantName) say there. Your bot ignores everyone but you.")
            }
        }
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if attempt.busy { ProgressView() } else {
                    Button("Connect") { Task { await attempt.connect(.telegram(botToken: token), api: model.api) } }
                        .disabled(token.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
    }

    private func pairing(_ c: ConnectionItem) -> some View {
        VStack(spacing: 20) {
            Spacer()
            IntegrationIcon(id: "telegram", size: 64)
            VStack(spacing: 6) {
                Text("One last step").font(.title3.weight(.semibold))
                Text("Open \(c.name) in Telegram and press **Start**. That makes it yours.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            if let action = c.actionUrl, let u = URL(string: action) {
                Button("Open \(c.name)") { openURL(u) }
                    .buttonStyle(SettingsInkButtonStyle())
            }
            HStack(spacing: 8) {
                ProgressView()
                Text("Waiting for you to press Start…").font(.footnote).foregroundStyle(.secondary)
            }
            Spacer()
        }
        .padding(24)
    }
}

// MARK: Email

private struct EmailConnect: View {
    let onDone: () -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var email = ""
    @State private var password = ""
    @State private var servers = MailServerSettings()
    /// Server settings the user changed by hand win over what was found.
    @State private var edited = false
    @State private var showServers = false
    @State private var found: MailServerDiscovery?
    @State private var foundFor = ""
    @State private var lookupError: String?
    @State private var searching = false
    @State private var attempt = ConnectAttempt()

    private var address: String { email.trimmingCharacters(in: .whitespaces).lowercased() }
    private var looksComplete: Bool { MailAddressCheck.looksComplete(address) }
    private var current: MailServerDiscovery? { foundFor == address ? found : nil }
    private var unsupported: Bool { current.map { !$0.supported } ?? false }
    private var useCustom: Bool { edited || (current != nil && current?.servers == nil && current?.supported == true) }
    private var ready: Bool {
        looksComplete && !password.isEmpty && !unsupported && !searching
            && (!useCustom || (!servers.imapHost.trimmingCharacters(in: .whitespaces).isEmpty && !servers.smtpHost.trimmingCharacters(in: .whitespaces).isEmpty))
    }

    var body: some View {
        Group {
            if let created = attempt.created {
                ConnectedView(connection: created, onDone: onDone)
            } else {
                form
            }
        }
        .navigationTitle("Email")
        .navigationBarTitleDisplayMode(.inline)
        // Work out the servers from the address, like mail apps do.
        .task(id: address) { await discover() }
    }

    private var form: some View {
        Form {
            Section {
                TextField("you@example.com", text: $email)
                    .keyboardType(.emailAddress)
                    .textContentType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .onChange(of: email) { _, _ in edited = false }
            } header: {
                Text("Email address")
            } footer: {
                discoveryLine
            }

            if unsupported {
                Section {
                    FormNote(warning: true, text: current?.help ?? "This mail service can't be connected.")
                }
            } else {
                Section {
                    SecureField(current?.needsAppPassword == true ? "App password" : "Password", text: $password)
                } footer: {
                    if let help = current?.help {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(help)
                            if let h = current?.helpUrl, let u = URL(string: h) {
                                Button("Open") { openURL(u) }.font(.footnote.weight(.semibold))
                            }
                        }
                    }
                }
                if looksComplete && !searching {
                    Section {
                        DisclosureGroup("Server settings", isExpanded: $showServers) {
                            serverFields
                        }
                    }
                }
            }
            if let e = attempt.error {
                Section { Text(e).font(.subheadline).foregroundStyle(Color.danger) }
            }
            Section {
                FormNote(text: "Your password is stored encrypted on your computer and only sent to your mail service. The last 90 days of mail are copied to your computer, so \(model.assistantName) can search and sort it without sending it anywhere else.")
            }
        }
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if attempt.busy {
                    HStack(spacing: 6) { ProgressView(); Text("Checking…").font(.footnote) }
                } else {
                    Button("Connect") { Task { await connect() } }.disabled(!ready)
                }
            }
        }
    }

    @ViewBuilder
    private var discoveryLine: some View {
        if searching {
            HStack(spacing: 6) { ProgressView().controlSize(.mini); Text("Looking up your mail settings…") }
        } else if let lookupError, foundFor == address {
            Text(lookupError).foregroundStyle(Color.danger)
        } else if let f = current, f.supported {
            if let s = f.servers {
                Label(f.provider ?? "Found your mail servers (\(s.imapHost))", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(Color.privateTone)
            } else {
                Text("Couldn't find the settings for this address. Enter them under Server settings, from your provider's help pages.")
            }
        }
    }

    @ViewBuilder
    private var serverFields: some View {
        serverRow("Incoming (IMAP)", host: binding(\.imapHost), port: binding(\.imapPort), security: binding(\.imapSecurity), placeholder: "imap.example.com")
        serverRow("Outgoing (SMTP)", host: binding(\.smtpHost), port: binding(\.smtpPort), security: binding(\.smtpSecurity), placeholder: "smtp.example.com")
        VStack(alignment: .leading, spacing: 4) {
            TextField("Username (\(address.isEmpty ? "your email address" : address))", text: Binding(
                get: { servers.username ?? "" },
                set: { edited = true; servers.username = $0.isEmpty ? nil : $0 }
            ))
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            Text("What you sign in to your mail with, not your name. Leave it empty to use your email address.")
                .font(.footnote).foregroundStyle(.secondary)
        }
    }

    private func serverRow(_ title: String, host: Binding<String>, port: Binding<Int>, security: Binding<MailSecuritySetting>, placeholder: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
            TextField(placeholder, text: host)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
            HStack {
                TextField("Port", value: port, format: .number.grouping(.never))
                    .keyboardType(.numberPad)
                    .frame(maxWidth: 80)
                Spacer()
                Picker("Encryption", selection: security) {
                    ForEach(MailSecuritySetting.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                .labelsHidden()
            }
        }
        .padding(.vertical, 2)
    }

    private func binding<T>(_ key: WritableKeyPath<MailServerSettings, T>) -> Binding<T> {
        Binding(get: { servers[keyPath: key] }, set: { edited = true; servers[keyPath: key] = $0 })
    }

    private func discover() async {
        guard looksComplete else {
            searching = false
            return
        }
        searching = true
        try? await Task.sleep(for: .milliseconds(500))
        guard !Task.isCancelled else { return }
        let asked = address
        do {
            let d = try await model.api?.discoverMail(asked)
            guard !Task.isCancelled else { return }
            found = d
            lookupError = nil
        } catch {
            guard !Task.isCancelled else { return }
            found = nil
            lookupError = error.localizedDescription
        }
        foundFor = asked
        searching = false
        // Pre-fill the server fields with what was found, unless the user typed their own.
        if !edited, let d = found {
            if let s = d.servers { servers = s }
            showServers = d.supported && d.servers == nil
        }
    }

    private func connect() async {
        guard ready else { return }
        let setup: ConnectionSetup = useCustom
            ? .email(email: email, password: password, preset: "other", servers: servers)
            : .email(email: email, password: password, preset: current?.preset, servers: current?.servers)
        await attempt.connect(setup, api: model.api)
    }
}

nonisolated enum MailAddressCheck {
    /// Looks like a whole address: something@domain.tld.
    static func looksComplete(_ s: String) -> Bool {
        s.range(of: #"^[^@\s]+@[^@\s]+\.[^@\s]{2,}$"#, options: .regularExpression) != nil
    }
}
