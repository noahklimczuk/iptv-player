import SwiftUI

/// Providers, and what the library holds.
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var adding = false
    /// Shown as a sheet (an iPhone), so it needs its own way out.
    var closable = false
    /// A section to open at, then cleared (Windows' `?section=`).
    var section: Binding<String?> = .constant(nil)
    @Environment(\.dismiss) private var dismiss

    /// The section the index marks: the first heading on screen, or the last one chosen.
    @State private var chosen = SettingsHead.profile
    @State private var onScreen: Set<String> = []
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    var body: some View {
        NavigationStack {
            ScrollViewReader { scroller in
            HStack(alignment: .top, spacing: 28) {
                // Windows' section index beside the page, where there is room for one.
                if wide && !locked {
                    SettingsIndex(titles: SettingsHead.order, current: current) { title in
                        chosen = title
                        withAnimation { scroller.scrollTo(title, anchor: .top) }
                    }
                }
                page
                .environment(\.settingsHeadSeen) { title, seen in
                    if seen { onScreen.insert(title) } else { onScreen.remove(title) }
                }
            }
            #if os(iOS)
            // The list keeps its own margin; the index beside it needs one to match.
            .padding(.leading, wide ? 20 : 0)
            #endif
            .frame(maxWidth: wide ? Self.pageWidth : .infinity)
            .frame(maxWidth: .infinity)
            // Built afresh by a switch between light and dark (`auroraRebuildOnSwitch`):
            // back to the picker that was just pressed, rather than the top.
            .onAppear {
                guard AuroraRebuild.returnToPicker else { return }
                AuroraRebuild.returnToPicker = false
                Task { scroller.scrollTo(AppearanceSection.id, anchor: .center) }
            }
            // Opening at a section: after the cards above it have laid out, or it
            // lands short, as on Windows.
            .task(id: section.wrappedValue) {
                guard let title = section.wrappedValue, SettingsHead.order.contains(title) else { return }
                try? await Task.sleep(for: .milliseconds(300))
                chosen = title
                scroller.scrollTo(title, anchor: .top)
                section.wrappedValue = nil
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

    /// The page itself. A list, except on a TV: a list there scrolls by focus alone and
    /// makes only the rows near the screen, so the index could not bring a section further
    /// down into view. A plain scroll view has every section made and scrolls on request.
    @ViewBuilder private var page: some View {
        #if os(tvOS)
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                sections
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 60)
            // A TV button centres a label that wraps; Windows' hints read from the left.
            .multilineTextAlignment(.leading)
        }
        // Right from the index lands in the page even where no control sits level with it.
        .focusSection()
        #else
        List {
            sections
        }
        #endif
    }

    @ViewBuilder private var sections: some View {
        Section {
            SettingsHead(SettingsHead.profile).id(SettingsHead.profile)
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
                    Image(systemName: "lock.fill").font(.ui(.largeTitle))
                    Text("Settings are locked").displayFont(.title2)
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

    /// Windows' `useCurrentSection`: the section being read. The one last chosen while its
    /// heading is on screen; otherwise the first heading on screen.
    private var current: String {
        if onScreen.contains(chosen) { return chosen }
        return SettingsHead.order.first { onScreen.contains($0) } ?? chosen
    }

    /// Windows' 1,180-pixel page; on a TV, the same page at ten feet.
    private static var pageWidth: CGFloat {
        #if os(tvOS)
        1640
        #else
        1180
        #endif
    }

    /// Room for the index: always on a TV, and on an iPad but not a phone.
    private var wide: Bool {
        #if os(tvOS)
        true
        #else
        sizeClass == .regular
        #endif
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
                Section {
                    SettingsHead("Providers").id("Providers")
                    ForEach(model.providers, id: \.id) { provider in
                        ProviderRow(provider: provider)
                    }
                    Button("Add provider", systemImage: "plus") { adding = true }
                        #if os(iOS)
                        .buttonStyle(SecondaryButtonStyle())
                        .listRowSeparator(.hidden)
                        #endif
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
                    SettingsHead("Parental controls").id("Parental controls")
                    NavigationLink {
                        ParentalControlsView()
                    } label: {
                        Label("Parental controls", systemImage: "lock")
                    }
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

/// One provider, as Windows' Settings shows it: a sunk tile in the Providers card with its
/// name, kind and counts, its connections and time left, and Refresh and Edit. A refresh
/// says what it is doing beside the button, then what it found.
private struct ProviderRow: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    let provider: Provider
    @State private var editing = false
    @State private var busy = false
    @State private var done: ImportReport?
    @State private var failure: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(provider.name).displayFont(.title3)
                Text("\(provider.kind.uppercased()) · \(provider.channelCount) channels · \(provider.movieCount) movies · \(provider.seriesCount) series")
                    .font(.ui(.subheadline))
                    .foregroundStyle(.secondary)
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 8) { badges; Spacer(minLength: 0); buttons }
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) { badges }
                    HStack(spacing: 8) { buttons }
                }
            }
            .modifier(Buttons())
            if let failure {
                Text(failure).font(.ui(.subheadline)).foregroundStyle(.red)
            } else if let line = status {
                Text(line).font(.ui(.subheadline)).foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 12)
        .padding(.horizontal, 18)
        .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: 16))
        #if os(iOS)
        .listRowSeparator(.hidden)
        .listRowInsets(EdgeInsets(top: 5, leading: 16, bottom: 5, trailing: 16))
        #endif
        .navigationDestination(isPresented: $editing) {
            ProviderEditorView(provider: provider)
        }
    }

    /// "0/2 connections" and "12 days left" (Windows' badges). Nothing is counting the
    /// streams in use yet, on either platform, so the first figure is 0.
    @ViewBuilder private var badges: some View {
        if let max = provider.maxConnections {
            Badge(text: "0/\(max) connections", style: .neutral)
        }
        if let days {
            Badge(text: days < 0 ? "Expired" : "\(days) days left", style: days < 7 ? .live : .new)
        }
    }

    @ViewBuilder private var buttons: some View {
        Button(busy ? "Refreshing…" : "Refresh", systemImage: "arrow.clockwise") {
            Task { await refresh() }
        }
        .disabled(busy || model.importProgress != nil)
        Button("Edit", systemImage: "pencil") { editing = true }
            .disabled(busy)
    }

    /// Windows' small secondary buttons, each taking its own taps rather than the row's.
    private struct Buttons: ViewModifier {
        func body(content: Content) -> some View {
            #if os(tvOS)
            content.buttonStyle(.bordered)
            #else
            content.buttonStyle(SecondaryButtonStyle())
            #endif
        }
    }



    private var days: Int? {
        provider.expiresAt.map { Int((Double($0) - Date().timeIntervalSince1970) / 86_400) }
    }

    /// The phase while refreshing; after, what it found. A count of zero is the most
    /// useful thing this can say: "0 channels" on a provider with twenty thousand films is
    /// a panel serving no live streams to this account, not a bug in Aurora.
    private var status: String? {
        if busy, let p = model.importProgress {
            return p.phase.short + (p.total > 0 ? " \(p.done)/\(p.total)" : "")
        }
        guard let done else { return nil }
        var line = "\(done.channels) channels · \(done.movies) movies · \(done.series) series"
        if done.dropped > 0 { line += " · \(done.dropped) skipped" }
        return line
    }

    private func refresh() async {
        busy = true
        done = nil
        failure = nil
        defer { busy = false }
        // Refresh what this provider already holds: films and series only if it has any,
        // so a live-only Apple TV library stays live-only.
        let content = ImportContent(live: true, vod: provider.movieCount > 0, series: provider.seriesCount > 0)
        do {
            done = try await model.importProvider(provider.id, content: content)
        } catch {
            // Said here rather than in the app-wide alert, which would take Settings
            // away with it on a phone (Windows reports "Could not refresh this provider").
            failure = "Could not refresh this provider. \(error.viewerMessage)"
        }
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
            SettingsHead("Filtering").id("Filtering")
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
                    Text(row.label).fontWeight(.heavy)
                    Text("\(row.counts.total.formatted()) in all · \(row.counts.nonEnglish.formatted()) not English · \(row.counts.untagged.formatted()) untagged · \(row.counts.duplicates.formatted()) duplicates")
                        .font(.ui(.caption)).foregroundStyle(.secondary)
                }
            }
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
                Text(title).fontWeight(.heavy)
                Text(hint).font(.ui(.caption)).foregroundStyle(.secondary)
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
            SettingsHead("Pause live TV").id("Pause live TV")
            if let settings {
                Toggle(isOn: Binding(get: { settings.enabled },
                                     set: { on in apply(TimeshiftChange(enabled: on, bytes: nil, secs: nil)) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Pause live TV").fontWeight(.heavy)
                        Text("Keeps the channel you are watching on disk so it can be paused and rewound.")
                            .font(.ui(.caption)).foregroundStyle(.secondary)
                    }
                }
                // Windows' rows of segments, the choice in its well.
                SettingsField(label: "Buffer size", hint: "The disk it may use.") {
                    Segments(options: choices.sizesGb.map { ($0 * Self.gib, "\($0) GB") },
                             selection: Binding(get: { settings.bytes },
                                                set: { apply(TimeshiftChange(enabled: nil, bytes: $0, secs: nil)) }))
                }
                .disabled(!settings.enabled)
                SettingsField(label: "Rewind limit",
                              hint: "How far back to offer. A gigabyte is eighteen minutes of HD and seven of 4K, so the shorter of the two is what you get.") {
                    Segments(options: choices.limitsMin.map { ($0 * 60, limitLabel($0 * 60)) },
                             selection: Binding(get: { settings.secs },
                                                set: { apply(TimeshiftChange(enabled: nil, bytes: nil, secs: $0)) }))
                }
                .disabled(!settings.enabled)
                if settings.reclaimableBytes > 0 {
                    LabeledContent {
                        Button("Empty buffer") { empty() }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Reclaimable: \(bytes(settings.reclaimableBytes))").fontWeight(.heavy)
                            Text("Left behind by earlier viewing. Emptying it loses nothing you are watching.")
                                .font(.ui(.caption)).foregroundStyle(.secondary)
                        }
                    }
                }
            }
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
            SettingsHead("Artwork and metadata").id("Artwork and metadata")
            Text("Posters, backdrops, descriptions and cast come from TMDB. Aurora only asks about a title once — if nothing matches confidently, it leaves the title alone rather than guessing, so a wrong poster never ends up on the wrong film.")
                .font(.ui(.caption)).foregroundStyle(.secondary)
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
                Text(message).font(.ui(.caption)).foregroundStyle(.secondary)
            }
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
                Text(extra.joined(separator: " · ")).font(.ui(.caption)).foregroundStyle(.secondary)
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
                .font(.ui(.caption)).foregroundStyle(.secondary)
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
    @Environment(\.auroraTheme) private var theme
    @State private var stats: LibraryStats?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    var body: some View {
        Section {
            SettingsHead("Library").id("Library")
            if let stats {
                // A plain grid, not a lazy one: a lazy grid inside a list row is measured
                // before its cells exist, and tvOS laid the next rows over the numbers.
                let tiles = [("Channels", stats.channels), ("Movies", stats.movies),
                             ("Series", stats.series), ("Episodes", stats.episodes)]
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 12) {
                    ForEach(Array(stride(from: 0, to: tiles.count, by: perRow)), id: \.self) { start in
                        GridRow {
                            ForEach(tiles[start..<min(start + perRow, tiles.count)], id: \.0) { label, value in
                                // Windows' `settings-tile`: a sunk well, the figure large.
                                VStack(alignment: .leading, spacing: 6) {
                                    Text(label).font(.ui(.subheadline).weight(.bold)).foregroundStyle(.secondary)
                                        .lineLimit(1).minimumScaleFactor(0.8)
                                    Text(value.formatted()).displayFont(.title2).monospacedDigit()
                                        .lineLimit(1).minimumScaleFactor(0.5)
                                }
                                .padding(.vertical, 16).padding(.horizontal, 18)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: 16))
                            }
                        }
                    }
                }
                .padding(.vertical, 4)
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("Guide coverage").fontWeight(.heavy)
                        Spacer()
                        Text("\(stats.guideMatched.formatted()) of \(stats.channels.formatted()) channels (\(percent(stats))%)")
                            .fontWeight(.bold).foregroundStyle(.secondary)
                    }
                    // Windows' `settings-meter`: an 8-point bar in the accent.
                    Capsule()
                        .fill(theme.surfaceSunk)
                        .frame(height: 8)
                        .overlay(alignment: .leading) {
                            GeometryReader { box in
                                Capsule().fill(theme.accent)
                                    .frame(width: box.size.width * CGFloat(percent(stats)) / 100)
                            }
                        }
                        .padding(.top, 4)
                    if !stats.guideUnmatched.isEmpty {
                        Text("No guide for: \(stats.guideUnmatched.joined(separator: ", "))")
                            .font(.ui(.caption)).foregroundStyle(.secondary)
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
        }
        .task(id: model.libraryVersion) {
            stats = try? await model.guarded { core in try core.libraryStats() }
        }
    }

    /// Windows' tiles fill the row at 150 pixels or more each: four across on a TV, two by
    /// two in the narrower column an iPad or a phone leaves beside the index.
    private var perRow: Int {
        #if os(tvOS)
        4
        #else
        2
        #endif
    }

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
