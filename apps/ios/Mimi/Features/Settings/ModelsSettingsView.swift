import SwiftUI

/// Everything the Models page knows, loaded together and kept fresh by the computer's
/// events (sources, downloads, the built-in runtime).
@Observable
final class ModelsData {
    var sources: [ModelSource] = []
    var presets: [ProviderPreset] = []
    var catalog: [CatalogModel] = []
    var rec: Recommendations?
    var runtime: RuntimeStatus?
    var pulls: [ModelPull] = []
    var models: [UUID: [ModelInfo]] = [:]
    var loading = true

    func load(_ api: MimiAPI?) async {
        guard let api else { return }
        async let s = api.modelSources()
        async let p = api.presets()
        async let c = api.catalog()
        async let r = api.recommendations()
        async let rt = api.runtime()
        async let pl = api.pulls()
        sources = (try? await s) ?? sources
        presets = (try? await p) ?? presets
        catalog = (try? await c) ?? catalog
        rec = (try? await r) ?? rec
        runtime = try? await rt
        pulls = (try? await pl) ?? pulls
        await withTaskGroup(of: (UUID, [ModelInfo]?).self) { group in
            for source in sources {
                group.addTask { (source.id, try? await api.models(source.id)) }
            }
            for await (id, list) in group {
                models[id] = list ?? []
            }
        }
        loading = false
    }

    func refreshPulls(_ api: MimiAPI?) async {
        guard let api, let list = try? await api.pulls() else { return }
        let finished = list.contains { p in p.state == .done && !pulls.contains { $0 == p } }
        pulls = list
        // A finished download is a new model in its source's list.
        if finished { await load(api) }
    }

    /// Every model from every source, in source order.
    var options: [ModelOption] {
        sources.flatMap { s in
            (models[s.id] ?? []).map { m in
                let info = ModelNames.info(m.id, sourceName: m.name, catalog: catalog)
                return ModelOption(ref: ModelRef(providerId: s.id, model: m.id), name: info.name, maker: info.maker,
                                   description: info.description, sourceName: s.name, locality: s.locality,
                                   sizeBytes: m.sizeBytes, supportsTools: m.supportsTools, price: m.price)
            }
        }
    }

    func preset(of s: ModelSource) -> ProviderPreset? {
        let bare = { (u: String) in u.hasSuffix("/") ? String(u.dropLast()) : u }
        return presets.first { $0.kind == s.kind && ($0.kind == .anthropic || bare($0.baseUrl) == bare(s.baseUrl)) }
    }

    func fits(_ ref: ModelRef) -> Bool { rec?.installed.first { $0.model == ref }?.fits ?? true }
}

/// Settings › Models: what the assistant thinks with, and where it runs. Downloads and the
/// built-in runtime happen on the computer; this page starts and follows them.
struct ModelsSettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var data = ModelsData()
    @State private var addingSource = false
    @State private var addingCloud = false
    @State private var editing: ModelSource?
    @State private var removingSource: ModelSource?
    @State private var removingModel: ModelOption?
    @State private var busy: String?
    @State private var error: String?

    private var current: ModelRef? { model.settings.defaultModel }
    private var inUse: ModelOption? { data.options.first { $0.ref == current } }

    var body: some View {
        List {
            activeSection
            computerSection
            detectedSection
            yourModelsSection
            suggestionsSection
            sourcesSection
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Models")
        .task(id: providersKey) { await data.load(model.api) }
        .task(id: model.revision("model_pull")) { await data.refreshPulls(model.api) }
        .task(id: model.revision("runtime_changed")) {
            if let rt = try? await model.api?.runtime() { data.runtime = rt }
        }
        .refreshable { await data.load(model.api) }
        .sheet(isPresented: $addingSource) { AddSourceSheet(onlyCloud: false, data: data) }
        .sheet(isPresented: $addingCloud) { AddSourceSheet(onlyCloud: true, data: data) }
        .sheet(item: $editing) { s in EditSourceSheet(source: s) }
        .alert("Remove \(removingSource?.name ?? "")?", isPresented: Binding(get: { removingSource != nil }, set: { if !$0 { removingSource = nil } })) {
            Button("Cancel", role: .cancel) { removingSource = nil }
            Button("Remove", role: .destructive) {
                guard let s = removingSource else { return }
                removingSource = nil
                Task { await run { try await model.api?.removeProvider(s.id) } }
            }
        } message: {
            Text("Its models won't be available anymore. Your conversations are kept.")
        }
        .alert("Remove \(removingModel?.name ?? "")?", isPresented: Binding(get: { removingModel != nil }, set: { if !$0 { removingModel = nil } })) {
            Button("Cancel", role: .cancel) { removingModel = nil }
            Button("Remove", role: .destructive) {
                guard let m = removingModel else { return }
                removingModel = nil
                Task {
                    await run { try await model.api?.deleteModel(m.ref.providerId, model: m.ref.model) }
                    await data.load(model.api)
                }
            }
        } message: {
            Text("It's deleted from your computer to free up space. You can download it again any time.")
        }
        .problemAlert($error)
    }

    /// Changes when sources are added, removed, renamed or move.
    private var providersKey: String {
        model.providers.map { "\($0.id)\($0.name)\($0.locality.rawValue)" }.joined(separator: "|")
    }

    // MARK: Sections

    private var activeSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top) {
                    SettingsIcon(systemImage: "sparkles", tint: .limeSoft, foreground: .limeDeep, size: 40)
                    Spacer()
                    if let locality = model.modelLocality { LocalityBadge(locality: locality) }
                }
                if let current {
                    let info: (name: String, description: String?) = inUse.map { ($0.name, $0.description) }
                        ?? (ModelNames.info(current.model, sourceName: nil, catalog: data.catalog).name, nil)
                    Text("Your assistant uses").font(.footnote.weight(.medium)).foregroundStyle(.secondary)
                    Text(info.name).font(.title2.weight(.semibold))
                    Text(info.description ?? "Your assistant's current model.")
                        .font(.subheadline).foregroundStyle(.secondary)
                } else {
                    Text("No model yet").font(.title3.weight(.semibold)).foregroundStyle(.secondary)
                    Text("Pick one of the models below, or connect a model source.")
                        .font(.subheadline).foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 6)
            NavigationLink {
                ModelPickerView(data: data, initialSource: nil)
            } label: {
                Text("Change model")
            }
            .disabled(data.options.isEmpty)
        }
    }

    @ViewBuilder
    private var computerSection: some View {
        if let rec = data.rec {
            let activeSize = model.modelLocality == .device ? inUse?.sizeBytes : nil
            let used = activeSize.map { min(1, Double($0) / Double(max(rec.modelBudgetBytes, 1))) } ?? 0
            let gpu = rec.hardware.gpus.first { $0.kind == "discrete" } ?? rec.hardware.gpus.first
            Section("Your computer") {
                VStack(alignment: .leading, spacing: 10) {
                    Text(rec.tierLine).font(.headline)
                    ProgressView(value: used).tint(Color.lime)
                    Text(activeSize != nil
                         ? "Your model uses about \(Int(used * 100))% of the room available."
                         : "Room for models up to about \(Int((Double(rec.modelBudgetBytes) / 1e9).rounded())) GB.")
                        .font(.footnote).foregroundStyle(.secondary)
                    Text("\(Int((Double(rec.hardware.totalMemoryBytes) / 1_073_741_824).rounded())) GB memory\(gpu.map { " · \($0.name)" } ?? "")")
                        .font(.footnote).foregroundStyle(.tertiary)
                }
                .padding(.vertical, 4)
            }
        }
    }

    @ViewBuilder
    private var detectedSection: some View {
        let servers = data.rec?.detectedServers.filter { !$0.alreadyAdded } ?? []
        if !servers.isEmpty {
            Section {
                ForEach(servers) { s in
                    ExplainedRow(systemImage: "dot.radiowaves.left.and.right", tint: .privateSoft, foreground: .privateTone,
                                 title: "\(s.name) is running on your computer",
                                 detail: "\(s.modelCount) model\(s.modelCount == 1 ? "" : "s") ready. Connect it to keep everything private.") {
                        Button("Connect") {
                            Task { await run(key: s.presetId) { _ = try await model.api?.addProvider(NewProvider(name: s.name, kind: .openaiCompatible, baseUrl: s.baseUrl)) } }
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(Color.ink)
                        .controlSize(.small)
                        .disabled(busy != nil)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var yourModelsSection: some View {
        let options = data.options
        let own = options.filter { $0.locality != .cloud && $0.ref != current }
        let cloud = data.sources.filter { $0.locality == .cloud }.compactMap { s -> (ModelSource, Int)? in
            let n = options.filter { $0.ref.providerId == s.id }.count
            return n > 0 ? (s, n) : nil
        }
        if !options.isEmpty || data.loading {
            Section {
                if let inUse { modelRow(inUse) }
                ForEach(own) { modelRow($0) }
                ForEach(cloud, id: \.0.id) { s, n in
                    NavigationLink {
                        ModelPickerView(data: data, initialSource: s.id)
                    } label: {
                        HStack(spacing: 12) {
                            SourceMark(presetId: data.preset(of: s)?.id, locality: s.locality, size: 30)
                            VStack(alignment: .leading, spacing: 1) {
                                Text("Choose from \(n) \(s.name) model\(n == 1 ? "" : "s")")
                                Text("Search, and see what each one costs").font(.footnote).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                if data.loading && options.isEmpty { ProgressView().frame(maxWidth: .infinity) }
            } header: {
                Text("Your models")
            }
        }
    }

    private func modelRow(_ o: ModelOption) -> some View {
        let isCurrent = o.ref == current
        let builtin = data.sources.first { $0.id == o.ref.providerId }?.kind == .builtin
        return Button {
            guard !isCurrent else { return }
            Task { await use(o.ref) }
        } label: {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(o.name).foregroundStyle(Color.primary)
                    Text(detail(o)).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                    if o.locality != .cloud && !data.fits(o.ref) {
                        Text("May be slow on your computer").font(.footnote).foregroundStyle(Color.cloudTone)
                    }
                }
                Spacer(minLength: 8)
                LocalityBadge(locality: o.locality, compact: true)
                Image(systemName: isCurrent ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(isCurrent ? Color.limeDeep : Color(.tertiaryLabel))
            }
        }
        .accessibilityAddTraits(isCurrent ? .isSelected : [])
        .contextMenu {
            if builtin {
                Button(isCurrent ? "In use, choose another first" : "Remove from your computer", systemImage: "trash", role: .destructive) {
                    removingModel = o
                }
                .disabled(isCurrent)
            }
        }
    }

    private func detail(_ o: ModelOption) -> String {
        if let d = o.description { return d }
        return [o.maker, o.price.map(ModelNames.costLabel), "From \(o.sourceName)"].compactMap { $0 }.joined(separator: " · ")
    }

    @ViewBuilder
    private var suggestionsSection: some View {
        if let rec = data.rec {
            Section {
                if rec.preferCloud { cloudRow(recommended: true) }
                ForEach(Array(rec.suggested.enumerated()), id: \.element.id) { i, m in
                    suggestedRow(m, best: i == 0 && !rec.preferCloud, providerId: rec.downloadProviderId)
                }
                if !rec.preferCloud { cloudRow(recommended: false) }
            } header: {
                Text("Good fits for your computer")
            } footer: {
                Text("Downloads go to your computer, and the models run there.")
            }
        }
    }

    private func suggestedRow(_ m: CatalogModel, best: Bool, providerId: UUID?) -> some View {
        let pull = data.pulls.first { $0.model == m.id && $0.providerId == providerId }
        let canStop = data.sources.first { $0.id == providerId }?.kind == .builtin
        let installed = data.options.contains { $0.ref.providerId == providerId && ($0.ref.model == m.id || $0.ref.model == "\(m.id):latest") }
        return VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(m.name).font(.headline)
                if best {
                    Text("Best fit")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.limeDeep)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .background(Color.limeSoft, in: .capsule)
                }
                Spacer()
            }
            Text(m.description).font(.subheadline).foregroundStyle(.secondary)
            if let pull, pull.state == .running {
                ProgressView(value: pull.fraction ?? 0).tint(Color.lime)
                HStack {
                    Text("\(pull.status)\(pull.fraction.map { " · \(Int($0 * 100))%" } ?? "")")
                        .font(.footnote).foregroundStyle(.secondary)
                    Spacer()
                    if canStop, let providerId {
                        Button("Stop") { Task { await run { try await model.api?.cancelPull(providerId, model: m.id) } } }
                            .font(.footnote.weight(.semibold))
                            .buttonStyle(.borderless)
                    }
                }
            } else if installed {
                Label("On your computer", systemImage: "checkmark.circle.fill")
                    .font(.footnote.weight(.medium)).foregroundStyle(Color.privateTone)
            } else {
                if let pull, pull.state == .failed, let e = pull.error {
                    Text(e).font(.footnote).foregroundStyle(Color.danger)
                } else if let pull, pull.state == .cancelled {
                    Text("Paused\(pull.fraction.map { " at \(Int($0 * 100))%" } ?? ""). It continues where it stopped.")
                        .font(.footnote).foregroundStyle(.secondary)
                }
                if let providerId {
                    Button {
                        Task { await download(m, providerId: providerId) }
                    } label: {
                        Label(pull?.state == .failed ? "Try again" : pull?.state == .cancelled ? "Resume"
                              : "Download to your computer · \(formatBytes(m.downloadBytes))",
                              systemImage: "arrow.down.circle")
                            .font(.subheadline.weight(.semibold))
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(best ? Color.lime : Color(.tertiarySystemFill))
                    .foregroundStyle(Color.ink)
                    .controlSize(.small)
                } else {
                    Text("To download models, install Ollama on your computer. Mimi finds it on its own.")
                        .font(.footnote).foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 4)
    }

    private func cloudRow(recommended: Bool) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Label("Cloud models", systemImage: "cloud.fill")
                    .font(.headline)
                    .foregroundStyle(Color.cloudTone)
                if recommended {
                    Text("Recommended")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.cloudTone)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .background(Color.cloudSoft, in: .capsule)
                }
            }
            Text("Smarter and faster, but your messages leave your computer.")
                .font(.subheadline).foregroundStyle(.secondary)
            Button("Add a service") { addingCloud = true }
                .font(.subheadline.weight(.semibold))
                .buttonStyle(.bordered)
                .tint(Color.cloudTone)
                .controlSize(.small)
        }
        .padding(.vertical, 4)
    }

    private var sourcesSection: some View {
        Section {
            ForEach(data.sources) { s in
                HStack(spacing: 12) {
                    SourceMark(presetId: data.preset(of: s)?.id, locality: s.locality, size: 32)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(s.name)
                        Text(sourceDetail(s)).font(.footnote).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    if s.locality == .cloud { LocalityBadge(locality: .cloud, compact: true) }
                }
                .swipeActions {
                    if s.kind != .builtin {
                        Button("Remove", systemImage: "trash") { removingSource = s }.tint(.red)
                        Button("Edit", systemImage: "pencil") { editing = s }.tint(.gray)
                    }
                }
                .contextMenu {
                    if s.kind != .builtin {
                        Button("Edit", systemImage: "pencil") { editing = s }
                        Button("Remove", systemImage: "trash", role: .destructive) { removingSource = s }
                    }
                }
            }
            Button { addingSource = true } label: {
                Label("Add a model source", systemImage: "plus")
            }
        } header: {
            Text("Model sources")
        }
    }

    private func sourceDetail(_ s: ModelSource) -> String {
        if s.kind == .builtin {
            let size = data.runtime?.modelsBytes ?? 0
            return "Runs models right on your computer\(size > 0 ? " · \(formatBytes(size)) of models" : "")"
        }
        switch s.locality {
        case .device: return "On your computer"
        case .network: return "On your network"
        case .cloud: return "Cloud service"
        }
    }

    // MARK: Actions

    private func use(_ ref: ModelRef) async {
        await run {
            try await model.updateSettings(["default_model": .object(["provider_id": .string(ref.providerId.mimiPath), "model": .string(ref.model)])])
        }
    }

    private func download(_ m: CatalogModel, providerId: UUID) async {
        await run(key: m.id) {
            if let started = try await model.api?.pull(providerId, model: m.id) {
                data.pulls.removeAll { $0.model == started.model && $0.providerId == started.providerId }
                data.pulls.append(started)
            }
        }
    }

    private func run(key: String = "", _ body: () async throws -> Void) async {
        busy = key
        defer { busy = nil }
        do { try await body() } catch { self.error = error.localizedDescription }
    }
}

/// Cloud services get a lettered tile in a tint of their own colour; things that run on
/// the user's machines keep the locality icon.
struct SourceMark: View {
    var presetId: String?
    var locality: Locality
    var size: CGFloat = 32

    var body: some View {
        if let mark = Self.marks[presetId ?? ""] {
            Text(mark.letter)
                .font(.system(size: size * 0.5, weight: .semibold))
                .foregroundStyle(Color(hex: mark.fg))
                .frame(width: size, height: size)
                .background(Color(hex: mark.bg), in: .rect(cornerRadius: size * 0.24, style: .continuous))
                .accessibilityHidden(true)
        } else {
            switch locality {
            case .device: SettingsIcon(systemImage: "desktopcomputer", tint: .privateSoft, foreground: .privateTone, size: size)
            case .network: SettingsIcon(systemImage: "house.fill", tint: .networkSoft, foreground: .networkTone, size: size)
            case .cloud: SettingsIcon(systemImage: "server.rack", tint: .cloudSoft, foreground: .cloudTone, size: size)
            }
        }
    }

    private static let marks: [String: (letter: String, bg: UInt32, fg: UInt32)] = [
        "anthropic": ("A", 0xF6E7DE, 0xB4532F),
        "openai": ("O", 0xE8ECE9, 0x1F2A24),
        "mistral": ("M", 0xFDEBD9, 0xD4610F),
        "openrouter": ("R", 0xE6E8FB, 0x4B52C8),
    ]
}
