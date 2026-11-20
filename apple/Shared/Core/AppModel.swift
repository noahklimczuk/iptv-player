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
    /// The providers have been asked for, however that went. Until then whether to show
    /// the setup wizard isn't known (Windows' `providersLoading`).
    private(set) var providersLoaded = false
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
        // Before the library opens, so a library started again is in the log too.
        LogExport.launchedAt = Date()
        installLog(sink: CoreLog())
        do {
            core = try AuroraCore.open(dataDir: Self.dataDirectory().path, secrets: KeychainSecretStore())
            // The rewind buffer in Caches: not backed up, and the system's to reclaim.
            let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            core?.useBufferFolder(path: caches.appendingPathComponent("timeshift", isDirectory: true).path)
            core?.useRecordingsFolder(path: Self.recordingsDirectory().path)
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

    /// Where recordings are written. Beside the library on an iPhone, as on Windows, but
    /// left out of the iCloud backup: a recording is gigabytes. An Apple TV keeps only
    /// Caches for anything that size, which the system may empty when space runs short
    /// (apple/README.md).
    private static func recordingsDirectory() -> URL {
        #if os(tvOS)
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Recordings", isDirectory: true)
        #else
        var folder = dataDirectory().appendingPathComponent("Recordings", isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? folder.setResourceValues(values)
        return folder
        #endif
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
        // A refusal ends the wait too: it has been reported, and the wizard is the right
        // answer to "no providers that we know of".
        providersLoaded = true
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
        do {
            _ = try await importProvider(providerId, content: content)
        } catch {
            self.error = error.viewerMessage
        }
    }

    /// Save a provider from the setup wizard's form, without importing it. Throws, so the
    /// wizard keeps the form and says why where it is.
    func saveProvider(_ draft: ProviderDraft) async throws -> Int64 {
        guard let core else { throw AuroraError.Other(message: "Aurora could not open its library.") }
        let id = try await Task.detached {
            if draft.kind == "xtream" {
                try core.addXtream(name: draft.name, baseUrl: draft.url,
                                   username: draft.username ?? "", password: draft.password ?? "")
            } else {
                try core.addM3u(name: draft.name, url: draft.url)
            }
        }.value
        await reload()
        return id
    }

    /// Import one provider and hand back what came of it. Throws rather than raising the
    /// app-wide alert, so the wizard can say it in place (Windows' `providers.refresh`).
    @discardableResult
    func importProvider(_ providerId: Int64, content: ImportContent) async throws -> ImportReport? {
        guard let core else { return nil }
        importProgress = ImportProgress(phase: .authenticating, done: 0, total: 0)
        let listener = ProgressRelay { [weak self] phase, done, total in
            Task { @MainActor in
                // Late reports must not bring the bar back once the import has ended.
                guard self?.importProgress != nil else { return }
                self?.importProgress = ImportProgress(phase: phase, done: done, total: total)
            }
        }
        let result = await Task.detached {
            Result { try core.refresh(providerId: providerId, content: content, listener: listener) }
        }.value
        importProgress = nil
        if case .success(let report) = result { lastImport = report }
        await reload()
        // An import leaves new titles bare; fetch their artwork and cast straight away,
        // as Windows does, rather than waiting for someone to find the button. Episode
        // listings likewise: an import stores each show without them.
        fetchMetadata()
        listEpisodes()
        return try result.get()
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

    // MARK: Recommendations (Gemini)

    /// Bumped when the Gemini switch or key changes, or Settings asked for a fresh set,
    /// so Home fetches its rail again.
    private(set) var aiVersion = 0

    func geminiStatus() async -> GeminiStatus? {
        let profile = profileId
        return await call(nil) { try $0.geminiStatus(profileId: profile) }
    }

    func setGeminiEnabled(_ on: Bool) async {
        await call(()) { try $0.setGeminiEnabled(enabled: on) }
        aiVersion += 1
    }

    /// Save the viewer's own key, or clear it with "" (the build's key, if any, is used).
    func setGeminiKey(_ key: String) async {
        await call(()) { try $0.setGeminiKey(key: key) }
        aiVersion += 1
    }

    /// The rail for Home. Quiet, as Windows' `AiRail`: every way this fails (no key,
    /// nothing watched, a refusal, nothing in this library) is a reason for the rail not
    /// to be there, and Settings is where it says why.
    func aiPicks() async -> AiPicks? {
        guard let core else { return nil }
        let profile = profileId
        return try? await Task.detached { try core.aiPicks(profileId: profile, refresh: false) }.value
    }

    /// A fresh set for Settings, with the error to show in place.
    func askForAiPicks() async -> Result<AiPicks, Error> {
        guard let core else { return .failure(AuroraError.Other(message: "The library is not open.")) }
        let profile = profileId
        let result = await Task.detached { Result { try core.aiPicks(profileId: profile, refresh: true) } }.value
        if case .success = result { aiVersion += 1 }
        return result
    }

    // MARK: Assistant

    /// A question to open the assistant with ("Ask about these" on the Gemini rail),
    /// taken by the assistant screen once it has asked it.
    var assistantQuestion: String?

    // MARK: Guide

    /// Bumped by the player's TV Guide button (Windows' G): the player closes and the
    /// Guide tab opens.
    private(set) var guideRequests = 0

    func openGuide() { guideRequests += 1 }

    // MARK: Search

    /// Text to open Search with ("Search this title" in the guide's info pane, Windows'
    /// `paletteQuery`), taken by the search screen as it opens.
    var searchQuery: String?

    func assistantStatus() async -> AssistantStatus? {
        let profile = profileId
        return await call(nil) { try $0.assistantStatus(profileId: profile) }
    }

    func assistantHistory() async -> [ChatMessage] {
        let profile = profileId
        return await call([]) { $0.assistantHistory(profileId: profile) }
    }

    func assistantClear() async {
        let profile = profileId
        await call(()) { $0.assistantClear(profileId: profile) }
    }

    /// Ask the assistant. The error comes back rather than going to the alert: the
    /// screen says it in the thread, as Windows does.
    func assistantSend(_ text: String, step: @escaping @MainActor (String) -> Void) async -> Result<SendResult, Error> {
        guard let core else { return .failure(AuroraError.Other(message: "The library is not open.")) }
        let profile = profileId
        let listener = AssistantSteps(step)
        return await Task.detached {
            Result { try core.assistantSend(profileId: profile, text: text, listener: listener) }
        }.value
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

    /// A page of the channels the guide's filters leave, and how many they leave in all.
    func guideChannels(_ filters: GuideFilters, limit: UInt32 = 200, offset: UInt32 = 0) async -> GuideChannels {
        let profile = profileId
        return await call(GuideChannels(channels: [], shown: 0)) {
            try $0.guideChannels(profileId: profile, filters: filters, limit: limit, offset: offset)
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

    /// One film by id, for a search hit to open; nil (and said) when it has gone.
    func movieDetail(id: Int64) async -> Movie? {
        await call(nil) { try $0.movieDetail(movieId: id) }
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

    // MARK: Playlist editor

    func playlistPage(_ query: PlaylistQuery, limit: UInt32, offset: UInt32) async -> PlaylistPage? {
        await call(nil) { try $0.playlistPage(query: query, limit: limit, offset: offset) }
    }

    func playlistGroups(_ kind: LibraryKind) async -> [PlaylistGroup] {
        await call([]) { try $0.playlistGroups(kind: kind) }
    }

    /// Every edit changes what the other screens list, so they start over.
    @discardableResult
    func playlistUpdate(_ kind: LibraryKind, id: Int64, patch: PlaylistPatch) async -> Bool {
        let done = await call(false) { try $0.playlistUpdate(kind: kind, id: id, patch: patch); return true }
        if done { libraryVersion += 1 }
        return done
    }

    @discardableResult
    func playlistSetHidden(_ kind: LibraryKind, ids: [Int64], hidden: Bool) async -> UInt32 {
        let changed = await call(0) { try $0.playlistSetHidden(kind: kind, ids: ids, hidden: hidden) }
        if changed > 0 { libraryVersion += 1 }
        return changed
    }

    @discardableResult
    func playlistHideMatching(_ query: PlaylistQuery, hidden: Bool) async -> UInt32 {
        let changed = await call(0) { try $0.playlistHideMatching(query: query, hidden: hidden) }
        if changed > 0 { libraryVersion += 1 }
        return changed
    }

    @discardableResult
    func playlistReset(_ kind: LibraryKind, ids: [Int64]) async -> UInt32 {
        let changed = await call(0) { try $0.playlistReset(kind: kind, ids: ids) }
        if changed > 0 { libraryVersion += 1 }
        return changed
    }

    func alternates(_ kind: LibraryKind, id: Int64) async -> [Alternate] {
        await call([]) { try $0.alternates(kind: kind, id: id) }
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

    // MARK: Editing a provider

    /// A provider's saved settings, password included, for the editor. Throws, so the
    /// editor can say why it has nothing to show.
    func providerSettings(_ id: Int64) async throws -> ProviderSettings {
        try await guarded { try $0.providerSettings(providerId: id) }
    }

    /// "Check connection": what the form holds, tried without saving it.
    func checkProvider(_ draft: ProviderDraft) async -> ProviderCheck? {
        guard let core else { return nil }
        return await Task.detached { core.checkProvider(draft: draft) }.value
    }

    /// Save edits to a provider. Throws, so the editor keeps the form and says why.
    func updateProvider(_ id: Int64, draft: ProviderDraft) async throws {
        try await guarded { try $0.updateProvider(providerId: id, draft: draft) }
        await reload()
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
            // Chosen through the core, as Windows does: what a profile may see is
            // decided there, so who is watching must be too.
            _ = await call(nil) { try $0.selectProfile(profileId: only.id, pin: nil) }
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

    /// Choose who is watching (`profiles.select`): the core checks the profile's PIN, if
    /// it has one, and applies its limits from now on. Returns what the PIN came to, or
    /// nil when it could not be asked; the profile is active only on `.ok` or
    /// `.notRequired`.
    @discardableResult
    func choose(_ profile: Profile, pin: String? = nil) async -> PinOutcome? {
        let outcome = await call(nil) { try $0.selectProfile(profileId: profile.id, pin: pin) }
        if outcome == .ok || outcome == .notRequired {
            activeProfile = profile
            profileId = profile.id
            picking = false
            await loadParental()
        }
        return outcome
    }

    // MARK: Parental controls (README §11, F-36 to F-38)

    /// The gate as it stands for whoever is watching (`parental_state`).
    private(set) var parental: ParentalState?
    /// The household's parental settings: the PIN, hide adult, lock Settings.
    private(set) var parentalSettings: ParentalSettings?
    /// What a parent locked, for the padlocks and Parental controls.
    private(set) var locks: [ParentalLock] = []

    /// Read the gate again. After every change and every PIN, as Windows' `useParental`.
    func loadParental() async {
        guard let core else { return }
        let read = try? await Task.detached {
            (try core.parentalState(), try core.parentalSettings(), try core.parentalLocks())
        }.value
        guard let read else { return }
        (parental, parentalSettings, locks) = read
    }

    /// Why the film and series pages are empty for this profile, when it is the age limit
    /// holding back every title without a rating, in a library that has titles: panels
    /// rarely send ratings (Windows' `BrowsePage`, F-36).
    var unratedHiddenReason: String? {
        guard let gate = parental, gate.unratedHidden, gate.ratedTitles < gate.titles else { return nil }
        return [
            "This profile only sees titles with a rating, and",
            gate.ratedTitles == 0
                ? "none of this library's are rated."
                : "\(gate.ratedTitles.formatted()) of \(gate.titles.formatted()) titles are rated.",
            "Your provider doesn't send ratings, so a parent can fetch them in Settings →",
            "Artwork and metadata, or allow unrated titles in Settings → Parental controls.",
        ].joined(separator: " ")
    }

    /// A channel is locked by itself or by the category it is filed under, and only while
    /// a parental PIN exists: without one the core applies no locks, so a padlock would
    /// be a lie (Windows' `isChannelLocked`).
    func isLocked(channelId: Int64, group: String?) -> Bool {
        guard parentalSettings?.hasMasterPin == true else { return false }
        let id = String(channelId)
        return locks.contains { lock in
            switch lock.kind {
            case .channel: lock.value == id
            case .category: group != nil && lock.value == group
            }
        }
    }

    /// The parental PIN being asked for: by the one dialog that answers every refusal
    /// (`ParentalPinHost`), as Windows' `ParentalPinDialog`.
    struct PinRequest: Identifiable {
        let id = UUID()
        let refusal: Refusal
        fileprivate let answer: CheckedContinuation<Bool, Never>
    }
    private(set) var pinRequest: PinRequest?
    /// The views able to show the dialog, the most recent on top: a sheet can only be
    /// presented from whatever is frontmost, so the newest host shows it.
    private(set) var pinHosts: [UUID] = []

    func addPinHost(_ id: UUID) { pinHosts.append(id) }
    func removePinHost(_ id: UUID) { pinHosts.removeAll { $0 == id } }

    /// Ask for the parental PIN. True once it has been entered and opened things.
    func askParentalPin(_ refusal: Refusal) async -> Bool {
        // One at a time: a second refusal while the dialog is up waits for its answer.
        if pinRequest != nil { return false }
        let opened = await withCheckedContinuation { answer in
            pinRequest = PinRequest(refusal: refusal, answer: answer)
        }
        if opened {
            await loadParental()
            // Hidden locked content may have just come into view.
            libraryVersion += 1
        }
        return opened
    }

    /// The dialog's answer.
    func answerPin(_ opened: Bool) {
        guard let request = pinRequest else { return }
        pinRequest = nil
        request.answer.resume(returning: opened)
    }

    func parentalUnlock(_ pin: String) async -> PinOutcome? {
        guard let core else { return nil }
        do {
            return try await Task.detached { try core.parentalUnlock(pin: pin) }.value
        } catch {
            self.error = error.viewerMessage
            return nil
        }
    }

    /// Close what the PIN opened, as when leaving Settings.
    func parentalRelock() async {
        guard parentalSettings?.hasMasterPin == true, let core else { return }
        parental = try? await Task.detached { try core.parentalRelock() }.value
    }

    /// Run a change to parental controls and read everything back, as Windows' panel's
    /// `run`. The PIN, if wanted, is asked for by `guarded`. Returns why it failed.
    func changeParental(_ body: @escaping @Sendable (AuroraCore) throws -> Void) async -> String? {
        var failure: String?
        do {
            try await guarded(body)
        } catch {
            failure = error.viewerMessage
        }
        await loadParental()
        await loadProfiles()
        libraryVersion += 1
        return failure
    }

    /// The daily limit's clock said time is up, and the player closed: say so.
    func viewingTimeUp() {
        error = "Today's viewing time is used up."
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

    // MARK: Recordings (README §7.7)

    /// Goes up after every scheduler tick that changed something: Windows' `dvr.tick`,
    /// which the Recordings screen, the guide's marks and the stream warning refresh on.
    private(set) var recordingsVersion = 0
    /// The guide's marks: what is scheduled or recording, reminders, and rules.
    private(set) var dvrMarks = DvrMarks()
    private var dvrStarted = false

    /// Start the scheduler's clock, once. Windows starts it when its window opens; here
    /// it runs while Aurora does.
    func startRecorder() {
        guard let core, !dvrStarted else { return }
        dvrStarted = true
        core.dvrStart(listener: DvrTicks { [weak self] _ in self?.recordingsChanged() })
    }

    /// Something about recordings changed: say so to every screen that shows them.
    func recordingsChanged() {
        recordingsVersion += 1
        Task { await loadDvrMarks() }
    }

    func loadDvrMarks() async {
        guard let core else { return }
        let marks = try? await Task.detached {
            DvrMarks(
                upcoming: try core.dvrList(state: .scheduled) + core.dvrList(state: .recording),
                reminders: try core.dvrReminders(),
                rules: try core.dvrRules())
        }.value
        if let marks { dvrMarks = marks }
    }

    /// Everything the Recordings screen draws, in one trip.
    func recordingsPage() async -> RecordingsPage? {
        await call(nil) { core in
            RecordingsPage(recordings: try core.dvrList(state: nil), rules: try core.dvrRules(),
                           conflicts: try core.dvrConflicts(), storage: try core.dvrStorage())
        }
    }

    func dvrStorage() async -> DvrStorage? {
        guard let core else { return nil }
        return try? await Task.detached { try core.dvrStorage() }.value
    }

    func recordingsInFlight() async -> [Recording] {
        guard let core else { return [] }
        return (try? await Task.detached { try core.dvrList(state: .recording) }.value) ?? []
    }

    /// Run a recordings change; nil when it worked, the reason when it did not. Every
    /// change is followed by a refresh, as Windows' `onChanged`.
    @discardableResult
    func changeRecordings<T: Sendable>(_ body: @escaping @Sendable (AuroraCore) throws -> T) async -> Result<T, Error> {
        do {
            let value = try await guarded(body)
            recordingsChanged()
            return .success(value)
        } catch {
            return .failure(error)
        }
    }

    /// Schedule an airing, or cancel the recording it has: the guide's Record button
    /// (`useDvrMarks.toggleRecord`).
    func toggleRecord(channelId: Int64, _ p: Programme) async -> String? {
        let existing = dvrMarks.recording(channelId: channelId, p)
        return await changeRecordings { core in
            if let existing {
                _ = try core.dvrCancel(id: existing.id)
            } else {
                _ = try core.dvrSchedule(recording: ScheduleRecording(
                    channelId: channelId, title: p.title, subTitle: p.subTitle, season: p.season,
                    episode: p.episode, airStart: p.start, airStop: p.stop,
                    prePaddingSecs: nil, postPaddingSecs: nil))
            }
        }.failureMessage
    }

    /// Remind me, or forget the reminder (`toggleReminder`).
    func toggleReminder(channelId: Int64, _ p: Programme) async -> String? {
        let existing = dvrMarks.reminder(channelId: channelId, p)
        return await changeRecordings { core in
            if let existing {
                _ = try core.dvrRemoveReminder(id: existing)
            } else {
                _ = try core.dvrAddReminder(channelId: channelId, title: p.title, start: p.start, leadSecs: nil)
            }
        }.failureMessage
    }

    /// Record series, or stop: a rule for this title on this channel (`toggleSeries`).
    func toggleSeries(channelId: Int64, _ p: Programme, anyChannel: Bool) async -> String? {
        let existing = dvrMarks.rule(p)
        return await changeRecordings { core in
            if let existing {
                _ = try core.dvrDeleteRule(id: existing)
            } else {
                // "This channel only" is the safer default, and `newOnly` false because
                // no guide this subscription sends marks anything new (R-133).
                _ = try core.dvrCreateRule(rule: NewSeriesRule(
                    title: p.title, channelId: anyChannel ? nil : channelId, newOnly: false,
                    weekdays: nil, aroundLocalMinute: nil, prePaddingSecs: nil,
                    postPaddingSecs: nil, keepEpisodes: nil))
            }
        }.failureMessage
    }

    /// Run one core call off the main actor; on failure, say why and return `fallback`.
    /// A refusal for want of the parental PIN asks for it and tries again (`guarded`).
    private func call<T: Sendable>(_ fallback: T, _ body: @escaping @Sendable (AuroraCore) throws -> T) async -> T {
        do {
            return try await guarded(body)
        } catch is CancellationError {
            return fallback
        } catch where error.wantsParentalPin {
            // The viewer was asked for the PIN and said never mind: nothing to report.
            return fallback
        } catch {
            self.error = error.viewerMessage
            return fallback
        }
    }

    /// Run one core call off the main actor. When the core refuses it for want of the
    /// parental PIN (a lock, the day's time, Settings or a guarded change), ask for the
    /// PIN and try again, as Windows' `invoke` does; cancelling the dialog throws the
    /// refusal. Every other failure is thrown as it came.
    /// Keep watch history to the window Settings → Privacy chose: two minutes after
    /// launch and every half hour after, as Windows' refresh sweep does.
    func runPrivacySweep() async {
        guard let core else { return }
        try? await Task.sleep(for: .seconds(120))
        while !Task.isCancelled {
            await Task.detached { core.privacyPrune() }.value
            try? await Task.sleep(for: .seconds(30 * 60))
        }
    }

    func guarded<T: Sendable>(_ body: @escaping @Sendable (AuroraCore) throws -> T) async throws -> T {
        guard let core else { throw CancellationError() }
        while true {
            do {
                return try await Task.detached { try body(core) }.value
            } catch where error.wantsParentalPin {
                guard let refusal = error.refusal, await askParentalPin(refusal) else { throw error }
            }
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

/// What the DVR already knows about the guide's airings (Windows' `useDvrMarks`): one
/// fetch, answered from lookups rather than a call per cell.
struct DvrMarks: Sendable {
    private var byAiring: [String: Recording] = [:]
    private var reminderKeys: [String: Int64] = [:]
    private var ruleTitles: [String: Int64] = [:]

    init() {}

    init(upcoming: [Recording], reminders: [Reminder], rules: [SeriesRule]) {
        for r in upcoming { byAiring[Self.key(r.channelId, r.title, r.airStart)] = r }
        for r in reminders { reminderKeys[Self.key(r.channelId, r.title, r.start)] = r.id }
        // Titles compared case-insensitively, as the core's match key does.
        for r in rules where r.enabled { ruleTitles[r.title.lowercased()] = r.id }
    }

    /// How the host identifies one airing: channel, airtime, title.
    private static func key(_ channelId: Int64, _ title: String, _ start: Int64) -> String {
        "\(channelId):\(start):\(title)"
    }

    func recording(channelId: Int64, _ p: Programme) -> Recording? {
        byAiring[Self.key(channelId, p.title, p.start)]
    }

    func reminder(channelId: Int64, _ p: Programme) -> Int64? {
        reminderKeys[Self.key(channelId, p.title, p.start)]
    }

    func rule(_ p: Programme) -> Int64? { ruleTitles[p.title.lowercased()] }
}

/// The Recordings screen's data, fetched together.
struct RecordingsPage: Sendable {
    var recordings: [Recording]
    var rules: [SeriesRule]
    var conflicts: [RecordingConflict]
    var storage: DvrStorage
}

extension Result {
    /// The viewer's words for a failure; nil when it worked.
    var failureMessage: String? {
        if case .failure(let error) = self { return error.viewerMessage }
        return nil
    }
}

/// Hands the scheduler's ticks to the main actor.
private final class DvrTicks: DvrListener, @unchecked Sendable {
    private let onTick: @MainActor (TickReport) -> Void
    init(_ onTick: @escaping @MainActor (TickReport) -> Void) { self.onTick = onTick }
    func ticked(report: TickReport) {
        Task { @MainActor in onTick(report) }
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

    /// Beside a provider's Refresh button (Windows' `PHASE_SHORT`).
    var short: String {
        switch self {
        case .authenticating: "Signing in"
        case .fetchingPlaylist: "Fetching list"
        case .importingChannels: "Channels"
        case .importingMovies: "Movies"
        case .importingSeries: "Series"
        case .fetchingEpg: "Guide"
        case .matchingEpg: "Matching guide"
        case .indexing: "Indexing"
        case .done: "Done"
        }
    }

    /// The setup wizard's sentence for it (Windows' `PHASE_SENTENCE`): it narrates a first
    /// run, where the label above sits beside a bar.
    var sentence: String {
        switch self {
        case .authenticating: "Checking your subscription"
        case .fetchingPlaylist: "Fetching your channel list"
        case .importingChannels: "Importing channels"
        case .importingMovies: "Importing films"
        case .importingSeries: "Importing series"
        case .fetchingEpg: "Downloading the TV guide"
        case .matchingEpg: "Matching the guide to your channels"
        case .indexing: "Building the search index"
        case .done: "Finished"
        }
    }
}

/// The core's log lines (`log.rs`), written into the system log beside mpv's, where
/// Settings → Diagnostics exports them from. The core redacts URLs before logging.
private final class CoreLog: LogSink {
    private let log = Logger(subsystem: "tv.aurora", category: "core")

    func line(level: UInt8, text: String) {
        switch level {
        case 0: log.error("\(text, privacy: .public)")
        case 1: log.warning("\(text, privacy: .public)")
        default: log.info("\(text, privacy: .public)")
        }
    }
}

/// The assistant's lookups as they happen, handed to the screen on the main actor.
private final class AssistantSteps: AssistantListener, @unchecked Sendable {
    private let onStep: @MainActor (String) -> Void
    init(_ onStep: @escaping @MainActor (String) -> Void) { self.onStep = onStep }
    func step(text: String) {
        Task { @MainActor in onStep(text) }
    }
}
