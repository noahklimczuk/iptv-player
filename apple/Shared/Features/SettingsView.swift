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
            ScrollViewReader { scroller in
            List {
                Section("Profile") {
                    LabeledContent("Watching as", value: model.activeProfile?.name ?? "")
                    Button("Profiles", systemImage: "person.2") { model.picking = true }
                }
                if locked {
                    // The core's answer (`settingsLocked`): a kids profile is kept out as
                    // well as everybody when the household locked Settings, and the core
                    // refuses what is behind them while it holds, so this is a courtesy
                    // rather than the lock. "Who's watching?" stays above it.
                    Section {
                        VStack(spacing: 12) {
                            Image(systemName: "lock.fill").font(.largeTitle)
                            Text("Settings are locked").font(.title2.bold())
                            Text("Enter the parental PIN to change them.").foregroundStyle(.secondary)
                            Button("Enter the PIN", systemImage: "lock.open") {
                                Task { _ = await model.askParentalPin(.pinRequired) }
                            }
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical)
                    }
                } else {
                    unlocked
                }
            }
            // Built afresh by a switch between light and dark (`auroraRebuildOnSwitch`):
            // back to the picker that was just pressed, rather than the top.
            .onAppear {
                guard AuroraRebuild.returnToPicker else { return }
                AuroraRebuild.returnToPicker = false
                Task { scroller.scrollTo(AppearanceSection.id, anchor: .center) }
            }
            }
            .auroraTitle("Settings")
            .toolbar {
                if closable {
                    ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                }
            }
            .sheet(isPresented: $adding) {
                SetupWizard(additional: true, onFinished: { adding = false }, onSkip: { adding = false })
                    .parentalPinHost()
                    .auroraAppearance()
            }
        }
        .parentalPinHost()
        .task { await model.loadParental() }
        // Leaving closes what the PIN opened, so the next person here is asked again
        // rather than inheriting the parent's half hour (Windows' `SettingsPage`).
        // Not when built afresh for a new scheme: that is not leaving.
        .onDisappear {
            guard !AuroraRebuild.inProgress else { return }
            Task { await model.parentalRelock() }
        }
    }

    /// Until the gate has answered, the household switch is the best guess, and the
    /// closed one.
    private var locked: Bool {
        if let state = model.parental { return state.settingsLocked }
        return model.parentalSettings.map { $0.lockSettings && $0.hasMasterPin } ?? false
    }

    /// Windows' sections, in Windows' order (`SettingsPage`): Providers, Appearance, Sync,
    /// Parental controls, Artwork and metadata, Recommendations, Privacy, Filtering,
    /// Pause live TV, Library, About, Diagnostics. Updates and Keyboard are not here:
    /// the App Store updates the app, and there is no keyboard (apple/README.md).
    @ViewBuilder private var unlocked: some View {
                Section("Providers") {
                    ForEach(model.providers, id: \.id) { provider in
                        ProviderRow(provider: provider)
                    }
                    Button("Add provider", systemImage: "plus") { adding = true }
                }
                if let progress = model.importProgress {
                    Section("Importing") {
                        Text(progress.phase.label)
                        if progress.total > 0 {
                            ProgressView(value: Double(progress.done), total: Double(progress.total))
                        }
                    }
                }
                AppearanceSection()
                SyncSection()
                Section {
                    NavigationLink {
                        ParentalControlsView()
                    } label: {
                        Label("Parental controls", systemImage: "lock")
                    }
                } header: {
                    Text("Parental controls")
                } footer: {
                    Text("What each profile may watch, and what needs the PIN.")
                }
                MetadataSection()
                RecommendationsSection()
                PrivacySection()
                LibraryFiltersSection()
                TimeshiftSection()
                LibraryStatsSection()
                AboutSection()
                DiagnosticsSection()
    }
}

private struct ProviderRow: View {
    @Environment(AppModel.self) private var model
    let provider: Provider
    @State private var editing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(provider.name).font(.headline)
            Text("\(provider.channelCount) channels · \(provider.movieCount) movies · \(provider.seriesCount) series")
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
            // Two buttons in one row: each takes its own taps (on tvOS, its own focus),
            // not the whole row's. With the default style tvOS focuses the row as one
            // white block that hides the provider's name.
            #if os(iOS)
            .buttonStyle(.borderless)
            #else
            .buttonStyle(.bordered)
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
            Text("Filtering")
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
        for (label, kind) in [("Channels", LibraryKind.channels), ("Movies", .movies), ("Series", .series)] {
            if let c = await model.filterCounts(kind), c.total > 0 {
                rows.append(KindCounts(label: label, counts: c))
            }
        }
        counts = rows
    }
}

/// Pause live TV (Windows' `TimeshiftPanel`, same wording): on or off, how much disk the
/// buffer may use and how far back it reaches, and emptying what an earlier run left.
/// No folder to choose: on Apple the buffer lives in the app's own cache folder.
private struct TimeshiftSection: View {
    @Environment(AppModel.self) private var model
    @State private var settings: TimeshiftSettings?
    @State private var message: String?
    private let choices = timeshiftChoices()
    private static let gib: UInt64 = 1024 * 1024 * 1024

    var body: some View {
        Section {
            if let settings {
                Toggle(isOn: Binding(get: { settings.enabled },
                                     set: { on in apply(TimeshiftChange(enabled: on, bytes: nil, secs: nil)) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Pause live TV")
                        Text("Keeps the channel you are watching on disk so it can be paused and rewound.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                Picker(selection: Binding(get: { settings.bytes },
                                          set: { apply(TimeshiftChange(enabled: nil, bytes: $0, secs: nil)) })) {
                    ForEach(choices.sizesGb, id: \.self) { gb in
                        Text("\(gb) GB").tag(gb * Self.gib)
                    }
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Buffer size")
                        Text("The disk it may use.").font(.caption).foregroundStyle(.secondary)
                    }
                }
                .disabled(!settings.enabled)
                Picker(selection: Binding(get: { settings.secs },
                                          set: { apply(TimeshiftChange(enabled: nil, bytes: nil, secs: $0)) })) {
                    ForEach(choices.limitsMin, id: \.self) { minutes in
                        Text(limitLabel(minutes * 60)).tag(minutes * 60)
                    }
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Rewind limit")
                        Text("How far back to offer. A gigabyte is eighteen minutes of HD and seven of 4K, so the shorter of the two is what you get.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                .disabled(!settings.enabled)
                if settings.reclaimableBytes > 0 {
                    LabeledContent {
                        Button("Empty buffer") { empty() }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Reclaimable: \(bytes(settings.reclaimableBytes))")
                            Text("Left behind by earlier viewing. Emptying it loses nothing you are watching.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
        } header: {
            Text("Pause live TV")
        } footer: {
            if let settings {
                Text((settings.enabled
                      ? "Up to \(limitLabel(settings.secs)) of rewind, within \(bytes(settings.bytes))."
                      : "Pausing live TV will freeze the picture and rejoin at the live edge.")
                     + (message.map { " " + $0 } ?? ""))
            }
        }
        .task { settings = try? model.core?.timeshiftSettings() }
    }

    private func apply(_ change: TimeshiftChange) {
        guard let core = model.core else { return }
        do {
            settings = try core.setTimeshift(change: change)
            message = "Saved. Applies to the next channel you tune."
        } catch {
            message = error.viewerMessage
        }
    }

    private func empty() {
        guard let core = model.core else { return }
        let freed = (try? core.emptyTimeshift()) ?? 0
        message = freed > 0 ? "Freed \(bytes(freed))." : "Nothing to empty."
        settings = try? core.timeshiftSettings()
    }

    /// "30 min", "3 hr" (Windows' `limitLabel`).
    private func limitLabel(_ secs: UInt32) -> String {
        let mins = Int((Double(secs) / 60).rounded())
        return mins % 60 == 0 && mins >= 60 ? "\(mins / 60) hr" : "\(mins) min"
    }

    private func bytes(_ n: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: n), countStyle: .binary)
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
                coverage("Movies", status.movies)
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
            Text("Artwork and metadata")
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

/// Settings → Library (Windows' tiles and guide coverage): what the library holds, and
/// how much of it the guide reaches. On an iPhone, where there is no Playlist tab, the
/// playlist editor opens from here.
private struct LibraryStatsSection: View {
    @Environment(AppModel.self) private var model
    @State private var stats: LibraryStats?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    var body: some View {
        Section {
            if let stats {
                // A plain grid, not a lazy one: a lazy grid inside a list row is measured
                // before its cells exist, and tvOS laid the next rows over the numbers.
                let tiles = [("Channels", stats.channels), ("Movies", stats.movies),
                             ("Series", stats.series), ("Episodes", stats.episodes)]
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 12) {
                    ForEach(Array(stride(from: 0, to: tiles.count, by: perRow)), id: \.self) { start in
                        GridRow {
                            ForEach(tiles[start..<min(start + perRow, tiles.count)], id: \.0) { label, value in
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(label).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                                    Text(value.formatted()).font(.title2.weight(.heavy)).monospacedDigit()
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                    }
                }
                .padding(.vertical, 4)
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("Guide coverage")
                        Spacer()
                        Text("\(stats.guideMatched.formatted()) of \(stats.channels.formatted()) channels (\(percent(stats))%)")
                            .fontWeight(.bold).foregroundStyle(.secondary)
                    }
                    ProgressView(value: Double(percent(stats)), total: 100)
                    if !stats.guideUnmatched.isEmpty {
                        Text("No guide for: \(stats.guideUnmatched.joined(separator: ", "))")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if compact {
                NavigationLink {
                    PlaylistEditorView()
                } label: {
                    Label("Playlist", systemImage: "list.bullet.rectangle")
                }
            }
        } header: {
            Text("Library")
        }
        .task(id: model.libraryVersion) {
            stats = try? await model.guarded { core in try core.libraryStats() }
        }
    }

    /// Windows' four tiles in a row where there is room; two by two on a phone.
    private var perRow: Int { compact ? 2 : 4 }

    private var compact: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }

    /// Windows' `epgPercent`.
    private func percent(_ s: LibraryStats) -> Int {
        s.channels > 0 ? Int((Double(s.guideMatched) / Double(s.channels) * 100).rounded()) : 0
    }
}
