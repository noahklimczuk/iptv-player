import SwiftUI

/// Providers, and what the library holds.
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var adding = false

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
            .sheet(isPresented: $adding) { NavigationStack { AddProviderView() } }
        }
    }
}

private struct ProviderRow: View {
    @Environment(AppModel.self) private var model
    let provider: Provider
    @State private var confirmingRemove = false

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
                Button("Remove", systemImage: "trash", role: .destructive) {
                    confirmingRemove = true
                }
            }
            .disabled(model.importProgress != nil)
        }
        .confirmationDialog("Remove \(provider.name)?", isPresented: $confirmingRemove, titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                Task { await model.removeProvider(provider.id) }
            }
        } message: {
            Text("Its channels, films, series and guide are removed from this device, and its password from the Keychain.")
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
