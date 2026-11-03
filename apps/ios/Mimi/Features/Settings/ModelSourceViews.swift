import SwiftUI

/// A searchable, filterable list of models to choose from: by source, those that can use
/// the calendar, mail and reminders, and cheapest first when prices are known.
struct ModelList: View {
    let entries: [ModelOption]
    let current: ModelRef?
    var sources: [ModelSource] = []
    var recommended: String?
    @Binding var source: UUID?
    let onChoose: (ModelOption) -> Void

    @State private var query = ""
    @State private var toolsOnly = true
    @State private var cheapest = false

    private var inSource: [ModelOption] { entries.filter { source == nil || $0.ref.providerId == source } }
    private var hasToolInfo: Bool { inSource.contains { $0.supportsTools == false } }
    private var hasPrices: Bool { inSource.contains { $0.price != nil } }

    private var shown: [ModelOption] {
        let words = query.lowercased().split(separator: " ").map(String.init)
        let list = inSource.filter { e in
            (!toolsOnly || !hasToolInfo || e.supportsTools != false)
                && words.allSatisfy { "\(e.name) \(e.maker ?? "") \(e.ref.model) \(e.sourceName)".lowercased().contains($0) }
        }
        func rank(_ e: ModelOption) -> Int {
            (e.ref.model == recommended && words.isEmpty ? 0 : 2) - (words.first.map { e.name.lowercased().hasPrefix($0) } == true ? 1 : 0)
        }
        return list.sorted { a, b in
            if rank(a) != rank(b) { return rank(a) < rank(b) }
            if cheapest {
                let ca = ModelNames.centsPerMessage(a.price), cb = ModelNames.centsPerMessage(b.price)
                if ca != cb { return ca < cb }
            }
            return a.name.localizedStandardCompare(b.name) == .orderedAscending
        }
    }

    var body: some View {
        let shown = shown
        let hidden = inSource.count - shown.count
        List {
            if sources.count > 1 || hasToolInfo || hasPrices {
                Section {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            if sources.count > 1 {
                                chip("All", active: source == nil) { source = nil }
                                ForEach(sources) { s in chip(s.name, active: source == s.id) { source = s.id } }
                            }
                            if hasToolInfo {
                                chip("Works with your calendar, mail and reminders", active: toolsOnly) { toolsOnly.toggle() }
                            }
                            if hasPrices {
                                chip("Cheapest first", active: cheapest) { cheapest.toggle() }
                            }
                        }
                        .padding(.horizontal, 16)
                    }
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
                }
            }
            Section {
                ForEach(shown) { e in
                    Button { onChoose(e) } label: { row(e) }
                }
            } footer: {
                Text("\(shown.count) model\(shown.count == 1 ? "" : "s")"
                     + (hidden > 0 && toolsOnly && hasToolInfo && query.isEmpty
                        ? " · \(hidden) that can't use your calendar, mail or reminders are hidden" : ""))
            }
        }
        .overlay {
            if shown.isEmpty {
                if query.isEmpty {
                    ContentUnavailableView("No models here", systemImage: "sparkles")
                } else {
                    ContentUnavailableView.search(text: query)
                }
            }
        }
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search by name or maker")
    }

    private func row(_ e: ModelOption) -> some View {
        let isCurrent = e.ref == current
        let details = [e.maker, sources.count > 1 && source == nil ? e.sourceName : nil,
                       e.price.map(ModelNames.costLabel), e.sizeBytes.map(formatBytes)].compactMap { $0 }
        return HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(e.name).foregroundStyle(Color.primary)
                    if e.ref.model == recommended {
                        Text("Recommended")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(Color.limeDeep)
                            .padding(.horizontal, 6).padding(.vertical, 2)
                            .background(Color.limeSoft, in: .capsule)
                    }
                }
                if !details.isEmpty {
                    Text(details.joined(separator: " · ")).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 8)
            LocalityBadge(locality: e.locality, compact: true)
            if isCurrent {
                Image(systemName: "checkmark").font(.body.weight(.semibold)).foregroundStyle(Color.limeDeep)
            }
        }
        .accessibilityAddTraits(isCurrent ? .isSelected : [])
    }

    private func chip(_ text: String, active: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(text)
                .font(.footnote.weight(.medium))
                .padding(.horizontal, 12)
                .frame(height: 30)
                .foregroundStyle(active ? Color.white : Color.ink)
                .background(active ? Color.ink : Color(.tertiarySystemFill), in: .capsule)
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

/// "Change model": every model from every source; choosing one makes it the assistant's.
struct ModelPickerView: View {
    let data: ModelsData
    let initialSource: UUID?
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var source: UUID?
    @State private var error: String?

    var body: some View {
        ModelList(entries: data.options, current: model.settings.defaultModel,
                  sources: data.sources.filter { !(data.models[$0.id] ?? []).isEmpty },
                  source: $source) { e in
            Task {
                do {
                    try await model.updateSettings(["default_model": .object([
                        "provider_id": .string(e.ref.providerId.mimiPath), "model": .string(e.ref.model),
                    ])])
                    dismiss()
                } catch {
                    self.error = error.localizedDescription
                }
            }
        }
        .navigationTitle("Choose a model")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { source = initialSource }
        .problemAlert($error)
    }
}

/// Adds a model source in three steps: where models run, connect (checked before anything
/// is saved), then the model to use.
struct AddSourceSheet: View {
    let onlyCloud: Bool
    let data: ModelsData
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ChooseSourceStep(onlyCloud: onlyCloud, presets: data.presets, catalog: data.catalog) { dismiss() }
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                }
        }
    }
}

private struct ChooseSourceStep: View {
    let onlyCloud: Bool
    let presets: [ProviderPreset]
    let catalog: [CatalogModel]
    let onDone: () -> Void

    var body: some View {
        List {
            if !onlyCloud {
                Section("On your computer") {
                    ForEach(presets.filter { $0.locality != .cloud }) { p in
                        NavigationLink { SourceDetailsStep(preset: p, catalog: catalog, onDone: onDone) } label: { pick(p) }
                    }
                    NavigationLink {
                        SourceDetailsStep(preset: nil, catalog: catalog, onDone: onDone)
                    } label: {
                        HStack(spacing: 12) {
                            SourceMark(locality: .network)
                            VStack(alignment: .leading, spacing: 1) {
                                Text("A server on your network")
                                Text("For example a computer with a big graphics card at home.").font(.footnote).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
            Section {
                ForEach(presets.filter { $0.locality == .cloud }) { p in
                    NavigationLink { SourceDetailsStep(preset: p, catalog: catalog, onDone: onDone) } label: { pick(p) }
                }
            } header: {
                Text(onlyCloud ? "Cloud services" : "Cloud services · these receive your messages")
            } footer: {
                if onlyCloud {
                    Text("Cloud models are smarter and faster, but your messages are sent to the service you pick.")
                }
            }
        }
        .navigationTitle(onlyCloud ? "Add a cloud service" : "Add a model source")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func pick(_ p: ProviderPreset) -> some View {
        HStack(spacing: 12) {
            SourceMark(presetId: p.id, locality: p.locality)
            VStack(alignment: .leading, spacing: 1) {
                Text(p.name)
                Text(p.description).font(.footnote).foregroundStyle(.secondary)
            }
        }
    }
}

private struct SourceDetailsStep: View {
    let preset: ProviderPreset?
    let catalog: [CatalogModel]
    let onDone: () -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var name = ""
    @State private var baseUrl = "http://"
    @State private var apiKey = ""
    @State private var busy = false
    @State private var error: String?
    @State private var probe: ProbeResult?

    private var locality: Locality { preset?.locality ?? .network }
    private var needsKey: Bool { preset?.needsApiKey ?? false }

    var body: some View {
        Form {
            Section {
                HStack(spacing: 12) {
                    SourceMark(presetId: preset?.id, locality: locality, size: 40)
                    Text(preset?.name ?? "A server on your network").font(.headline)
                    Spacer()
                    LocalityBadge(locality: locality)
                }
                .listRowBackground(Color.clear)
            } footer: {
                if let preset, locality == .cloud {
                    Text("Your messages, and what \(model.assistantName) reads to answer them (like emails or notes), are sent to \(preset.name). \(preset.name) charges your account for what you use.")
                } else if let preset {
                    Text("Make sure \(preset.name) is open on your computer, then connect. Nothing leaves your computer.")
                }
            }
            if preset == nil {
                Section {
                    TextField("Name, like Home server", text: $name)
                    TextField("Address", text: $baseUrl)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } footer: {
                    Text("The address your computer uses to reach the server.")
                }
            }
            if needsKey || preset == nil {
                Section {
                    SecureField(needsKey ? "Paste your key" : "API key, only if the server needs one", text: $apiKey)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } header: {
                    Text(needsKey ? "API key" : "API key (optional)")
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Kept encrypted on your computer, and never shown again.")
                        if let key = preset?.keyUrl, let u = URL(string: key) {
                            Button("Get a key from \(preset?.name ?? "")") { openURL(u) }
                                .font(.footnote.weight(.semibold))
                        }
                    }
                }
            }
            if let error {
                Section { Text(error).foregroundStyle(Color.danger).font(.subheadline) }
            }
        }
        .navigationTitle("Connect")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if busy { ProgressView() } else {
                    Button("Connect") { Task { await connect() } }
                        .disabled(needsKey && apiKey.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        .navigationDestination(item: $probe) { result in
            SourceModelStep(preset: preset, name: name, apiKey: apiKey, probe: result, catalog: catalog, onDone: onDone)
        }
        .onAppear {
            if let preset {
                name = preset.name
                baseUrl = preset.baseUrl
            }
        }
    }

    private func connect() async {
        guard let api = model.api else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            let key = apiKey.trimmingCharacters(in: .whitespaces)
            probe = try await api.probe(ProbeRequest(kind: preset?.kind ?? .openaiCompatible, baseUrl: baseUrl, apiKey: key.isEmpty ? nil : key))
        } catch {
            self.error = error.localizedDescription
        }
    }
}

private struct SourceModelStep: View {
    let preset: ProviderPreset?
    let name: String
    let apiKey: String
    let probe: ProbeResult
    let catalog: [CatalogModel]
    let onDone: () -> Void
    @Environment(AppModel.self) private var model
    @State private var busy = false
    @State private var error: String?
    @State private var source: UUID?

    private var locality: Locality { preset?.locality ?? probe.locality }
    private static let placeholder = UUID(uuidString: "00000000-0000-0000-0000-000000000000")!

    /// Not saved yet, so the entries point at a placeholder source.
    private var entries: [ModelOption] {
        probe.models.map { m in
            let info = ModelNames.info(m.id, sourceName: m.name, catalog: catalog)
            return ModelOption(ref: ModelRef(providerId: Self.placeholder, model: m.id), name: info.name, maker: info.maker,
                               description: info.description, sourceName: preset?.name ?? name, locality: locality,
                               sizeBytes: m.sizeBytes, supportsTools: m.supportsTools, price: m.price)
        }
    }

    var body: some View {
        Group {
            if probe.models.isEmpty {
                ContentUnavailableView {
                    Label("Connected, but no models yet", systemImage: "checkmark.circle")
                } description: {
                    Text("Add some there, then choose one in Models.")
                } actions: {
                    Button("Add it anyway") { Task { await add(use: nil) } }.buttonStyle(.borderedProminent).tint(Color.ink)
                }
            } else {
                ModelList(entries: entries, current: nil, recommended: preset?.recommendedModel, source: $source) { e in
                    Task { await add(use: e) }
                }
                .safeAreaInset(edge: .top) {
                    Label("Connected · \(probe.models.count) model\(probe.models.count == 1 ? "" : "s") available. Choose the one to use.",
                          systemImage: "checkmark.circle.fill")
                        .font(.footnote.weight(.medium))
                        .foregroundStyle(Color.privateTone)
                        .padding(.horizontal, 14).padding(.vertical, 10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.privateSoft, in: .rect(cornerRadius: 12))
                        .padding(.horizontal, 16)
                }
            }
        }
        .navigationTitle(preset?.name ?? "Choose a model")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if model.settings.defaultModel != nil && !probe.models.isEmpty {
                ToolbarItem(placement: .confirmationAction) {
                    if busy { ProgressView() } else {
                        Button("Add without switching") { Task { await add(use: nil) } }
                    }
                }
            }
        }
        .problemAlert($error)
    }

    private func add(use entry: ModelOption?) async {
        guard let api = model.api else { return }
        busy = true
        defer { busy = false }
        do {
            let key = apiKey.trimmingCharacters(in: .whitespaces)
            let host = URL(string: probe.baseUrl)?.host() ?? probe.baseUrl
            let trimmed = name.trimmingCharacters(in: .whitespaces)
            let provider = try await api.addProvider(NewProvider(
                name: trimmed.isEmpty ? host : trimmed, kind: preset?.kind ?? .openaiCompatible,
                baseUrl: probe.baseUrl, apiKey: key.isEmpty ? nil : key))
            if let entry {
                try await model.updateSettings(["default_model": .object([
                    "provider_id": .string(provider.id.mimiPath), "model": .string(entry.ref.model),
                ])])
            }
            onDone()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// A model source's name, address, key and where it runs.
struct EditSourceSheet: View {
    let source: ModelSource
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var baseUrl = ""
    @State private var apiKey = ""
    @State private var locality: Locality = .device
    @State private var error: String?

    /// Anthropic has one address, in the cloud; only the name and key can change.
    private var fixed: Bool { source.kind == .anthropic }

    var body: some View {
        NavigationStack {
            Form {
                Section("Name") { TextField("Name", text: $name) }
                if !fixed {
                    Section("Address") {
                        TextField("Address", text: $baseUrl)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                    }
                }
                Section("API key") {
                    SecureField(source.hasApiKey ? "Leave empty to keep the saved key" : "None", text: $apiKey)
                }
                if !fixed {
                    Section {
                        Picker("Where it runs", selection: $locality) {
                            Text("On your computer").tag(Locality.device)
                            Text("On your network").tag(Locality.network)
                            Text("Cloud").tag(Locality.cloud)
                        }
                    } footer: {
                        Text("Worked out from the address. Change it only for your own server behind a public name.")
                    }
                }
                if let error {
                    Section { Text(error).foregroundStyle(Color.danger).font(.subheadline) }
                }
            }
            .navigationTitle(source.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { Task { await save() } }
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear {
                name = source.name
                baseUrl = source.baseUrl
                locality = source.locality
            }
        }
    }

    private func save() async {
        do {
            _ = try await model.api?.updateProvider(source.id, ProviderUpdate(
                name: name,
                baseUrl: baseUrl == source.baseUrl ? nil : baseUrl,
                apiKey: apiKey.isEmpty ? nil : apiKey,
                locality: locality
            ))
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
