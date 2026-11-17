import Foundation
import Observation
import os

/// The app's one handle on the Rust core, and the state the screens draw from.
///
/// Every core call blocks — an import takes minutes, a tune a moment — so each one runs
/// off the main actor and only its result comes back to it. The same rule as Windows,
/// where every command runs on Tauri's blocking pool (docs/ARCHITECTURE.md, Threading).
@MainActor
@Observable
final class AppModel {
    private(set) var providers: [Provider] = []
    private(set) var groups: [ChannelGroup] = []
    private(set) var importProgress: ImportProgress?
    private(set) var lastImport: ImportReport?
    /// Goes up whenever the library may have changed (an import, a provider removed).
    /// Screens that load once key their loading on it, so a finished import shows up
    /// without leaving the screen.
    private(set) var libraryVersion = 0
    /// The last thing that failed, for the app-wide alert (`RootView`). Also logged:
    /// the core redacts credentials before building these messages (`error.rs`), so the
    /// log is a safe place for them, and a scripted run can see a failure without a
    /// screenshot.
    var error: String? {
        didSet { if let error { Self.log.error("\(error, privacy: .public)") } }
    }
    private static let log = Logger(subsystem: "tv.aurora", category: "error")

    /// Whose favourites, My List and progress these are. Falls back to 1, which exists in
    /// every library (an `aurora-db` migration creates it), as on Windows.
    private(set) var profileId: Int64 = 1
    private(set) var profiles: [Profile] = []
    private(set) var activeProfile: Profile?
    /// "Who's watching?" is up. True until a profile is chosen, as on Windows.
    var picking = true
    /// The picker's question has been asked of the core, however it was answered. Until
    /// then the first screen isn't known, so nothing is shown rather than the wrong one.
    private(set) var profilesLoaded = false

    /// `nil` only if the library could not be opened, and then `error` says why.
    let core: AuroraCore?

    struct ImportProgress: Equatable {
        var phase: ImportPhase
        var done: UInt64
        var total: UInt64
    }

    /// A metadata sweep in progress: titles looked up in this batch, out of how many.
    struct MetadataProgress: Equatable {
        var done: UInt64
        var total: UInt64
    }
    private(set) var metadataProgress: MetadataProgress?
    private var metadataTask: Task<Void, Never>?
    private var listingTask: Task<Void, Never>?

    init() {
        do {
            core = try AuroraCore.open(dataDir: Self.dataDirectory().path, secrets: KeychainSecretStore())
        } catch {
            core = nil
            self.error = "Aurora could not open its library. \(error.viewerMessage)"
        }
        // Windows starts both sweeps at launch too: whatever the last ones didn't reach.
        Task {
            self.fetchMetadata()
            self.listEpisodes()
        }
    }

    /// Application Support, which the system does not purge — unlike Caches, which is
    /// where an Apple TV keeps almost everything else (docs §2.3).
    private static func dataDirectory() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Aurora", isDirectory: true)
    }

    func reload() async {
        guard let core else { return }
        do {
            let (providers, groups) = try await Task.detached {
                (try core.providers(), try core.channelGroups())
            }.value
            // Closing the player reloads too; only a real change (an import's counts and
            // time, a provider added or removed) should make screens start over.
            if providers != self.providers { libraryVersion += 1 }
            self.providers = providers
            self.groups = groups
        } catch {
            self.error = error.viewerMessage
        }
    }

    /// Save an Xtream panel and import it straight away.
    func addXtream(name: String, url: String, username: String, password: String, content: ImportContent) async {
        guard let core else { return }
        do {
            let id = try await Task.detached {
                try core.addXtream(name: name, baseUrl: url, username: username, password: password)
            }.value
            await refresh(providerId: id, content: content)
        } catch {
            self.error = error.viewerMessage
        }
    }

    func addM3u(name: String, url: String, content: ImportContent) async {
        guard let core else { return }
        do {
            let id = try await Task.detached { try core.addM3u(name: name, url: url) }.value
            await refresh(providerId: id, content: content)
        } catch {
            self.error = error.viewerMessage
        }
    }

    func refresh(providerId: Int64, content: ImportContent) async {
        guard let core else { return }
        importProgress = ImportProgress(phase: .authenticating, done: 0, total: 0)
        defer { importProgress = nil }
        let listener = ProgressRelay { [weak self] phase, done, total in
            Task { @MainActor in
                self?.importProgress = ImportProgress(phase: phase, done: done, total: total)
            }
        }
        do {
            lastImport = try await Task.detached {
                try core.refresh(providerId: providerId, content: content, listener: listener)
            }.value
        } catch {
            self.error = error.viewerMessage
        }
        await reload()
        // An import leaves new titles bare; fetch their artwork and cast straight away,
        // as Windows does, rather than waiting for someone to find the button. Episode
        // listings likewise: an import stores each show without them.
        fetchMetadata()
        listEpisodes()
    }

    /// Fill in the episode listings an import left out, in the background and quietly,
    /// as Windows' `sweep_in_background` does. Opening a show fetches its own listing
    /// regardless; this is what gives the rest their seasons.
    func listEpisodes() {
        guard let core, listingTask == nil else { return }
        let relay = MetadataRelay { _, _ in }
        listingTask = Task {
            defer { listingTask = nil }
            do {
                let report = try await withTaskCancellationHandler {
                    try await Task.detached { try core.listEpisodes(listener: relay) }.value
                } onCancel: {
                    relay.cancel()
                }
                Self.log.notice("episode listings: asked \(report.asked) listed \(report.listed) failed \(report.failed)")
            } catch {
                Self.log.notice("episode listing sweep stopped: \(error.viewerMessage, privacy: .public)")
            }
        }
    }

    // MARK: Metadata

    /// Fetch artwork, cast and ratings for whatever is still bare, in the background.
    ///
    /// Started on its own after launch and after every import, where it must never get in
    /// the way: no key, no network, a failure, all stay quiet, as on Windows. Started from
    /// Settings (`userAsked`), a failure is said.
    func fetchMetadata(userAsked: Bool = false) {
        guard let core, metadataTask == nil else { return }
        let relay = MetadataRelay { [weak self] done, total in
            Task { @MainActor in self?.metadataProgress = MetadataProgress(done: done, total: total) }
        }
        metadataTask = Task {
            defer {
                metadataTask = nil
                metadataProgress = nil
            }
            if !userAsked {
                let status = try? await Task.detached { try core.metadataStatus() }.value
                guard let status, status.hasKey, status.movies.pending + status.series.pending > 0 else { return }
            }
            do {
                let report = try await withTaskCancellationHandler {
                    try await Task.detached { try core.fetchMetadata(listener: relay) }.value
                } onCancel: {
                    relay.cancel()
                }
                // New genres, posters and ratings: lists and facets reload.
                if report.matched > 0 { libraryVersion += 1 }
            } catch {
                if userAsked { self.error = error.viewerMessage }
                Self.log.notice("metadata sweep stopped: \(error.viewerMessage, privacy: .public)")
            }
        }
    }

    /// Stop a sweep after the titles in flight; what was fetched is kept.
    func stopMetadata() {
        metadataTask?.cancel()
    }

    func metadataStatus() async -> MetadataStatus? {
        await call(nil) { try $0.metadataStatus() }
    }

    /// Save the viewer's own TMDB key, or clear it with nil.
    func setMetadataKey(_ key: String?) async {
        await call(()) { try $0.setMetadataKey(key: key) }
    }

    func credits(_ kind: ListKind, id: Int64) async -> [Credit] {
        await call([]) { try $0.credits(kind: kind, id: id) }
    }

    /// Forget a wrong match and look the title up again.
    func rematch(_ kind: ListKind, id: Int64) async {
        await call(()) { try $0.rematch(kind: kind, id: id) }
        fetchMetadata(userAsked: true)
    }

    /// `list` is which favourites list `favoritesOnly` shows and each channel's
    /// `favorite` answers for: the default Favourites when nil.
    func channels(group: String?, favoritesOnly: Bool = false, list: String? = nil,
                  limit: UInt32 = 200, offset: UInt32 = 0) async -> [Channel] {
        let profile = profileId
        return await call([]) {
            try $0.channels(profileId: profile, group: group, favoritesOnly: favoritesOnly,
                            favoritesList: list, limit: limit, offset: offset)
        }
    }

    /// Into or out of `list` (the default Favourites when nil); a new name makes a new
    /// list. Returns whether the channel is in that list now.
    func toggleFavorite(channelId: Int64, list: String? = nil) async -> Bool? {
        let profile = profileId
        return await call(nil) { try $0.toggleFavorite(profileId: profile, channelId: channelId, list: list) }
    }

    /// Favourites first (always there), then the named lists alphabetically.
    func favoriteLists() async -> [FavoriteList] {
        let profile = profileId
        return await call([]) { try $0.favoriteLists(profileId: profile) }
    }

    func renameFavoriteList(_ from: String, to: String) async {
        let profile = profileId
        _ = await call(0) { try $0.renameFavoriteList(profileId: profile, from: from, to: to) }
    }

    func deleteFavoriteList(_ name: String) async {
        let profile = profileId
        _ = await call(0) { try $0.deleteFavoriteList(profileId: profile, name: name) }
    }

    /// Save a list in the order on screen.
    func reorderFavorites(list: String?, channelIds: [Int64]) async {
        let profile = profileId
        _ = await call(0) { try $0.reorderFavorites(profileId: profile, list: list, channelIds: channelIds) }
    }

    /// The guide grid for the channels on screen, over `from..to`.
    func guideRows(channelIds: [Int64], from: Int64, to: Int64) async -> [GuideRow] {
        await call([]) { try $0.guideRows(channelIds: channelIds, from: from, to: to) }
    }

    /// Which of these programmes can be replayed now, by id.
    func replayable(channelId: Int64, _ programmes: [Programme]) async -> Set<Int64> {
        let starts = programmes.map(\.start)
        let flags = await call([]) { try $0.catchUpAvailable(channelId: channelId, starts: starts) }
        return Set(zip(programmes, flags).filter(\.1).map(\.0.id))
    }

    func search(_ text: String) async -> SearchResults? {
        await call(nil) { try $0.search(text: text, limit: 30) }
    }

    func nowNext(channelIds: [Int64]) async -> [NowNext] {
        await call([]) { try $0.nowNext(channelIds: channelIds) }
    }

    func schedule(channelId: Int64, from: Date, to: Date) async -> [Programme] {
        let from = Int64(from.timeIntervalSince1970), to = Int64(to.timeIntervalSince1970)
        return await call([]) { try $0.schedule(channelId: channelId, from: from, to: to) }
    }

    func movies(_ browse: Browse) async -> [Movie] {
        await call([]) { try $0.movies(browse: browse) }
    }

    /// The shelves and genres films can be narrowed by, and how many `browse` matches.
    func movieFacets(_ browse: Browse) async -> BrowseFacets? {
        await call(nil) { try $0.movieFacets(browse: browse) }
    }

    func series(_ browse: Browse) async -> [Series] {
        await call([]) { try $0.seriesList(browse: browse) }
    }

    func seriesDetail(seriesId: Int64) async -> Series? {
        await call(nil) { try $0.seriesDetail(seriesId: seriesId) }
    }

    func seriesFacets(_ browse: Browse) async -> BrowseFacets? {
        await call(nil) { try $0.seriesFacets(browse: browse) }
    }

    // MARK: Library filters

    func libraryFilters() async -> LibraryFilters? {
        await call(nil) { try $0.libraryFilters() }
    }

    /// Save the library-wide filters. Every list reads them, so every screen starts over.
    func setLibraryFilters(_ filters: LibraryFilters) async -> LibraryFilters? {
        let saved = await call(nil) { try $0.setLibraryFilters(filters: filters) }
        if saved != nil { libraryVersion += 1 }
        return saved
    }

    func filterCounts(_ kind: LibraryKind) async -> FilterCounts? {
        await call(nil) { try $0.filterCounts(kind: kind) }
    }

    func episodes(seriesId: Int64, season: UInt16?) async -> [Episode] {
        await call([]) { try $0.episodes(seriesId: seriesId, season: season) }
    }

    func progress(_ kind: VodKind, id: Int64) async -> WatchProgress? {
        let profile = profileId
        return await call(nil) { try $0.watchProgress(profileId: profile, kind: kind, id: id) }
    }

    func seriesResume(seriesId: Int64) async -> SeriesResume? {
        let profile = profileId
        return await call(nil) { try $0.seriesResume(profileId: profile, seriesId: seriesId) }
    }

    /// Remove a provider and everything imported from it.
    func removeProvider(_ id: Int64) async {
        _ = await call(false) { try $0.removeProvider(providerId: id) }
        await reload()
    }

    // MARK: Profiles

    /// Load the profiles, by Windows' rule (`state/profile.ts`): with exactly one, and no
    /// PIN on it, there is nobody to choose between, so it is chosen -- but only at
    /// launch, or opening the picker to add a profile would close it again at once.
    /// Otherwise the active profile is refreshed (a rename shows at once), and if it was
    /// deleted there is none until one is chosen.
    func loadProfiles() async {
        let atLaunch = !profilesLoaded
        let all = await call([]) { try $0.profiles() }
        profiles = all
        let still = activeProfile.flatMap { active in all.first { $0.id == active.id } }
        if let still {
            activeProfile = still
        } else if all.count == 1, let only = all.first, !only.hasPin {
            activeProfile = only
            profileId = only.id
            if atLaunch { picking = false }
        } else {
            activeProfile = nil
            picking = true
        }
        profilesLoaded = true
    }

    /// Add a profile; a kids profile starts limited to age 12, as on Windows.
    func createProfile(name: String, isKids: Bool, pin: String?) async -> Bool {
        let ok = await call(false) { _ = try $0.createProfile(name: name, isKids: isKids, pin: pin); return true }
        await loadProfiles()
        return ok
    }

    func updateProfile(_ id: Int64, name: String, pin: PinChange) async -> Bool {
        let ok = await call(false) { try $0.updateProfile(profileId: id, name: name, pin: pin); return true }
        await loadProfiles()
        return ok
    }

    /// False for the last profile, which can't be deleted.
    func deleteProfile(_ id: Int64) async -> Bool {
        let gone = await call(false) { try $0.deleteProfile(profileId: id) }
        if !gone && error == nil { error = "The last profile cannot be deleted." }
        await loadProfiles()
        return gone
    }

    func activate(_ profile: Profile) {
        activeProfile = profile
        profileId = profile.id
        picking = false
    }

    func verifyPin(_ profile: Profile, pin: String) async -> PinOutcome? {
        await call(nil) { try $0.verifyProfilePin(profileId: profile.id, pin: pin) }
    }

    // MARK: My List

    /// Returns whether it is on My List now.
    func toggleMyList(_ kind: ListKind, id: Int64) async -> Bool? {
        let profile = profileId
        return await call(nil) { try $0.toggleMyList(profileId: profile, kind: kind, id: id) }
    }

    /// Returns whether it is liked now.
    func toggleLiked(_ kind: ListKind, id: Int64) async -> Bool? {
        let profile = profileId
        return await call(nil) { try $0.toggleLiked(profileId: profile, kind: kind, id: id) }
    }

    func marks(_ kind: ListKind, id: Int64) async -> ItemMarks? {
        let profile = profileId
        return await call(nil) { try $0.itemMarks(profileId: profile, kind: kind, id: id) }
    }

    func myList() async -> MyList? {
        let profile = profileId
        return await call(nil) { try $0.myList(profileId: profile) }
    }

    // MARK: Home

    /// The home feed's shuffle for this run of the app, as Windows keeps one per session:
    /// the same rails while the app is open, a different mix next time.
    let homeSeed = UInt64(Date().timeIntervalSince1970)

    func homeRails() async -> [Rail] {
        let profile = profileId, seed = homeSeed
        return await call([]) { try $0.homeRails(profileId: profile, seed: seed) }
    }

    func railPage(_ railId: String, offset: Int) async -> RailPage? {
        let profile = profileId, seed = homeSeed
        return await call(nil) {
            try $0.homeRailPage(profileId: profile, railId: railId, offset: UInt32(offset), limit: nil, seed: seed)
        }
    }

    /// Returns whether it is hidden now.
    func setNotInterested(_ kind: ListKind, id: Int64, _ on: Bool = true) async -> Bool? {
        let profile = profileId
        return await call(nil) { try $0.setNotInterested(profileId: profile, kind: kind, id: id, on: on) }
    }

    /// Take a film or show off Continue Watching. Returns whether anything went.
    func forgetProgress(_ kind: ListKind, id: Int64) async -> Bool? {
        let profile = profileId
        return await call(nil) { try $0.forgetProgress(profileId: profile, kind: kind, id: id) }
    }

    /// Fetch TMDB trending and "also liked" lists where they are due. Quiet, as on
    /// Windows: a failure only means the feed is built from what is already kept.
    func refreshHomeSignals() async -> UInt32 {
        guard let core else { return 0 }
        let profile = profileId
        return (try? await Task.detached { try core.refreshHomeSignals(profileId: profile) }.value) ?? 0
    }

    /// Run one core call off the main actor; on failure, say why and return `fallback`.
    private func call<T: Sendable>(_ fallback: T, _ body: @escaping @Sendable (AuroraCore) throws -> T) async -> T {
        guard let core else { return fallback }
        do {
            return try await Task.detached { try body(core) }.value
        } catch {
            self.error = error.viewerMessage
            return fallback
        }
    }
}

/// What a film or series screen is narrowed by: the browse side of Windows'
/// `useBrowse`. My List is a different list altogether, not a filter on this one.
struct BrowseFilters: Equatable {
    var sort: BrowseSort = .recentlyAdded
    var shelf: String?
    var genre: String?
    var letter: String?
    var search = ""
    var myList = false

    /// How many titles one page holds, as on Windows.
    static let pageSize: UInt32 = 120

    func browse(offset: UInt32 = 0) -> Browse {
        Browse(sort: sort, category: shelf, genre: genre, query: search, letter: letter,
               limit: Self.pageSize, offset: offset)
    }
}

extension BrowseSort {
    static let all: [BrowseSort] = [.recentlyAdded, .title, .year, .rating]

    var label: String {
        switch self {
        case .recentlyAdded: "Recently added"
        case .title: "A–Z"
        case .year: "Year"
        case .rating: "Rating"
        }
    }
}

/// Hands a sweep's progress to a closure, and tells the core when to stop. Serves both
/// the metadata and the episode listing sweeps, which report the same way.
private final class MetadataRelay: MetadataListener, ListingListener, @unchecked Sendable {
    private let onProgress: @Sendable (UInt64, UInt64) -> Void
    private let stopped = OSAllocatedUnfairLock(initialState: false)

    init(_ onProgress: @escaping @Sendable (UInt64, UInt64) -> Void) {
        self.onProgress = onProgress
    }

    func cancel() { stopped.withLock { $0 = true } }

    func progress(done: UInt64, total: UInt64) -> Bool {
        onProgress(done, total)
        return !stopped.withLock { $0 }
    }
}

/// Hands import progress from the importing thread to a closure.
private final class ProgressRelay: ImportListener, @unchecked Sendable {
    private let onProgress: @Sendable (ImportPhase, UInt64, UInt64) -> Void

    init(_ onProgress: @escaping @Sendable (ImportPhase, UInt64, UInt64) -> Void) {
        self.onProgress = onProgress
    }

    func progress(phase: ImportPhase, done: UInt64, total: UInt64) {
        onProgress(phase, done, total)
    }
}

extension ImportPhase {
    /// What the progress line says.
    var label: String {
        switch self {
        case .authenticating: "Signing in"
        case .fetchingPlaylist: "Downloading the playlist"
        case .importingChannels: "Importing channels"
        case .importingMovies: "Importing films"
        case .importingSeries: "Importing series"
        case .fetchingEpg: "Downloading the guide"
        case .matchingEpg: "Matching the guide to channels"
        case .indexing: "Building search"
        case .done: "Done"
        }
    }
}
