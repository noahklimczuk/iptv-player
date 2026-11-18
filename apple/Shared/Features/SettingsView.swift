import SwiftUI

/// Providers, and what the library holds.
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var adding = false
    /// Shown as a sheet (an iPhone), so it needs its own way out.
    var closable = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("Profile") {
                    LabeledContent("Watching as", value: model.activeProfile?.name ?? "")
                    Button("Profiles", systemImage: "person.2") { model.picking = true }
                }
                Section("Providers") {
                    ForEach(model.providers, id: \.id) { provider in
                        ProviderRow(provider: provider)
                    }
                    Button("Add a provider", systemImage: "plus") { adding = true }
                }
                LibraryFiltersSection()
                Section {
                    NavigationLink {
                        PlaylistEditorView()
                    } label: {
                        Label("Edit the playlist", systemImage: "list.bullet.rectangle")
                    }
                } footer: {
                    Text("Rename, renumber, regroup or hide channels, films and series, and put them back.")
                }
                MetadataSection()
                if let progress = model.importProgress {
                    Section("Importing") {
                        Text(progress.phase.label)
                        if progress.total > 0 {
                            ProgressView(value: Double(progress.done), total: Double(progress.total))
                        }
                    }
                }
                Section("About") {
                    LabeledContent("Aurora core", value: coreVersion())
                    if let core = model.core {
                        LabeledContent("Library", value: core.dataDir())
                            .font(.caption)
                    }
                }
            }
            .navigationTitle("Settings")
            .toolbar {
                if closable {
                    ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                }
            }
            .sheet(isPresented: $adding) { NavigationStack { AddProviderView() } }
        }
    }
}

private struct ProviderRow: View {
    @Environment(AppModel.self) private var model
    let provider: Provider
    @State private var editing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(provider.name).font(.headline)
            Text("\(provider.channelCount) channels · \(provider.movieCount) films · \(provider.seriesCount) series")
                .font(.caption).foregroundStyle(.secondary)
            if let at = provider.lastRefreshAt {
                Text("Refreshed \(Date(timeIntervalSince1970: TimeInterval(at)), style: .relative) ago")
                    .font(.caption2).foregroundStyle(.tertiary)
            }
            if let max = provider.maxConnections {
                Text("\(max) stream\(max == 1 ? "" : "s") at once").font(.caption2).foregroundStyle(.tertiary)
            }
            HStack {
                Button("Refresh", systemImage: "arrow.clockwise") {
                    Task { await refresh() }
                }
                // Change the address or sign-in, check it, or remove the provider
                // (Windows' Edit opens the same editor).
                Button("Edit", systemImage: "pencil") { editing = true }
            }
            #if os(iOS)
            // Two buttons in one row: each takes its own taps, not the whole row's.
            .buttonStyle(.borderless)
            #endif
            .disabled(model.importProgress != nil)
        }
        .navigationDestination(isPresented: $editing) {
            ProviderEditorView(provider: provider)
        }
    }

    private func refresh() async {
        // Refresh what this provider already holds: films and series only if it has any,
        // so a live-only Apple TV library stays live-only.
        let content = ImportContent(live: true, vod: provider.movieCount > 0, series: provider.seriesCount > 0)
        await model.refresh(providerId: provider.id, content: content)
    }
}

/// The library-wide filters (Windows' `FilterPanel`). Each says what it would hide before
/// it is turned on: on a big playlist "English only" is either a relief or empties the
/// library, and the difference can't be guessed.
private struct LibraryFiltersSection: View {
    @Environment(AppModel.self) private var model
    @State private var filters: LibraryFilters?
    @State private var counts: [KindCounts] = []
    @State private var saving = false

    private struct KindCounts: Identifiable {
        let label: String
        let counts: FilterCounts
        var id: String { label }
    }

    var body: some View {
        Section {
            if let filters {
                toggle("Show English content only",
                       hint: "Hides channels and titles the provider tagged as another language. Anything untagged is kept: most playlists label only some of what they carry.",
                       isOn: filters.englishOnly) { $0.englishOnly = $1 }
                toggle("Also hide anything untagged",
                       hint: "Hides what the provider didn't tag as well. The counts below say how much that is; on a playlist that tags nothing, it's everything.",
                       isOn: filters.englishOnly && filters.hideUntagged) { $0.hideUntagged = $1 }
                    .disabled(!filters.englishOnly)
                toggle("Collapse duplicates",
                       hint: "Shows one entry per title, keeping the highest quality copy.",
                       isOn: filters.hideDuplicates) { $0.hideDuplicates = $1 }
            }
            ForEach(counts) { row in
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.label)
                    Text("\(row.counts.total.formatted()) in all · \(row.counts.nonEnglish.formatted()) not English · \(row.counts.untagged.formatted()) untagged · \(row.counts.duplicates.formatted()) duplicates")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        } header: {
            Text("Library filters")
        }
        .disabled(saving)
        .task(id: model.libraryVersion) { await load() }
    }

    private func toggle(_ title: String, hint: String, isOn: Bool,
                        set: @escaping (inout LibraryFilters, Bool) -> Void) -> some View {
        Toggle(isOn: Binding(
            get: { isOn },
            set: { on in
                guard var next = filters else { return }
                set(&next, on)
                Task { await save(next) }
            }
        )) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                Text(hint).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func save(_ next: LibraryFilters) async {
        saving = true
        defer { saving = false }
        // The saved answer, not the request: what the lists will now apply.
        if let saved = await model.setLibraryFilters(next) { filters = saved }
    }

    private func load() async {
        filters = await model.libraryFilters()
        var rows: [KindCounts] = []
        for (label, kind) in [("Channels", LibraryKind.channels), ("Films", .movies), ("Series", .series)] {
            if let c = await model.filterCounts(kind), c.total > 0 {
                rows.append(KindCounts(label: label, counts: c))
            }
        }
        counts = rows
    }
}

/// Posters, cast and ratings from TMDB (Windows' `MetadataPanel`, same wording). The build
/// normally carries a key, so the usual state is nothing to do but watch the counts.
private struct MetadataSection: View {
    @Environment(AppModel.self) private var model
    @State private var status: MetadataStatus?
    @State private var key = ""
    @State private var message: String?

    var body: some View {
        Section {
            Text("Posters, backdrops, descriptions and cast come from TMDB. Aurora only asks about a title once — if nothing matches confidently, it leaves the title alone rather than guessing, so a wrong poster never ends up on the wrong film.")
                .font(.caption).foregroundStyle(.secondary)
            if let status {
                coverage("Films", status.movies)
                coverage("Series", status.series)
                if let progress = model.metadataProgress {
                    VStack(alignment: .leading) {
                        Text("Fetching… \(progress.done.formatted()) of \(progress.total.formatted())")
                        ProgressView(value: Double(progress.done), total: Double(max(progress.total, 1)))
                    }
                    Button("Stop", systemImage: "stop.fill") { model.stopMetadata() }
                } else if status.hasKey {
                    Button("Fetch metadata now", systemImage: "arrow.down.circle") {
                        message = nil
                        model.fetchMetadata(userAsked: true)
                    }
                }
                keyRows(status)
            }
            if let message {
                Text(message).font(.caption).foregroundStyle(.secondary)
            }
        } header: {
            Text("Metadata")
        }
        .task(id: ReloadKey(library: model.libraryVersion, running: model.metadataProgress != nil)) {
            status = await model.metadataStatus()
        }
    }

    private struct ReloadKey: Equatable {
        let library: Int
        let running: Bool
    }

    private func coverage(_ label: String, _ c: Coverage) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                Spacer()
                Text("\(c.matched.formatted()) of \(c.total.formatted())").foregroundStyle(.secondary)
            }
            // Not found is a settled answer, not an error, and is labelled as one.
            let extra = [c.noMatch > 0 ? "\(c.noMatch.formatted()) not found" : nil,
                         c.failed > 0 ? "\(c.failed.formatted()) to retry" : nil].compactMap { $0 }
            if !extra.isEmpty {
                Text(extra.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
            }
            ProgressView(value: Double(c.matched), total: Double(max(c.total, 1)))
        }
    }

    @ViewBuilder
    private func keyRows(_ status: MetadataStatus) -> some View {
        SecureField(status.keyIsBuiltIn ? "Built in — nothing to enter"
                    : status.hasKey ? "•••••••• (a key is saved)" : "Paste your TMDB API key",
                    text: $key)
            .textContentType(.password)
            .autocorrectionDisabled()
            .onSubmit { Task { await save(key) } }
        if !key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Button("Save key") { Task { await save(key) } }
        }
        if status.hasKey && !status.keyIsBuiltIn {
            Button("Remove key", role: .destructive) { Task { await save(nil) } }
        }
        if status.keyIsBuiltIn {
            Text("This build ships with a metadata key, so artwork and cast work without any setup. Entering your own replaces it — worth doing only if you would rather the requests counted against your own rate limit than one shared with every copy of this build.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private func save(_ value: String?) async {
        await model.setMetadataKey(value)
        key = ""
        message = value == nil ? "Key removed." : "Key saved."
        status = await model.metadataStatus()
        if value != nil { model.fetchMetadata(userAsked: true) }
    }
}
