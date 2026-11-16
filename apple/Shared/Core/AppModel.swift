import Foundation
import Observation

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
    var error: String?

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

    init() {
        do {
            core = try AuroraCore.open(dataDir: Self.dataDirectory().path, secrets: KeychainSecretStore())
        } catch {
            core = nil
            self.error = "Aurora could not open its library. \(error.localizedDescription)"
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
            self.providers = providers
            self.groups = groups
        } catch {
            self.error = error.localizedDescription
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
            self.error = error.localizedDescription
        }
    }

    func addM3u(name: String, url: String, content: ImportContent) async {
        guard let core else { return }
        do {
            let id = try await Task.detached { try core.addM3u(name: name, url: url) }.value
            await refresh(providerId: id, content: content)
        } catch {
            self.error = error.localizedDescription
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
            self.error = error.localizedDescription
        }
        await reload()
    }

    func channels(group: String?, favoritesOnly: Bool = false, limit: UInt32 = 200, offset: UInt32 = 0) async -> [Channel] {
        let profile = profileId
        return await call([]) {
            try $0.channels(profileId: profile, group: group, favoritesOnly: favoritesOnly, limit: limit, offset: offset)
        }
    }

    /// Returns whether the channel is a favourite now.
    func toggleFavorite(channelId: Int64) async -> Bool? {
        let profile = profileId
        return await call(nil) { try $0.toggleFavorite(profileId: profile, channelId: channelId) }
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

    func movieShelves() async -> [Shelf] {
        await call([]) { try $0.movieShelves() }
    }

    func series(_ browse: Browse) async -> [Series] {
        await call([]) { try $0.seriesList(browse: browse) }
    }

    func seriesDetail(seriesId: Int64) async -> Series? {
        await call(nil) { try $0.seriesDetail(seriesId: seriesId) }
    }

    func seriesShelves() async -> [Shelf] {
        await call([]) { try $0.seriesShelves() }
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

    /// Run one core call off the main actor; on failure, say why and return `fallback`.
    private func call<T: Sendable>(_ fallback: T, _ body: @escaping @Sendable (AuroraCore) throws -> T) async -> T {
        guard let core else { return fallback }
        do {
            return try await Task.detached { try body(core) }.value
        } catch {
            self.error = error.localizedDescription
            return fallback
        }
    }
}

extension Browse {
    /// The first page of a shelf, newest first.
    static func shelf(_ category: String?, query: String? = nil, offset: UInt32 = 0) -> Browse {
        Browse(sort: .recentlyAdded, category: category, query: query, limit: 120, offset: offset)
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
