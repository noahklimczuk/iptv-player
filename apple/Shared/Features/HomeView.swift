import SwiftUI

/// The home screen, as Windows' `HomePage`: Continue Watching first, then the rails the
/// core's feed builds (My List, For you, Top 10, Because you watched…, the shelves).
///
/// Behaviour matches Windows: each rail pages as it is scrolled, a suggestion can be
/// hidden with "Not interested" (gone at once, put back if the core says no), a title
/// can be taken off Continue Watching, and after the first load the TMDB lists the feed
/// reads are refreshed once, rebuilding the rails if any came back.
///
/// The Gemini rail ("Because of what you watch") is loaded on its own, after Continue
/// Watching, as Windows' `AiRail`: it can take seconds where the rest is a database
/// query, and it shows nothing at all until it has something.
///
/// The page opens on Windows' bento: the featured billboard (`HeroBillboard`) beside
/// "On now" (`OnNowTile`).
struct HomeView: View {
    @Environment(AppModel.self) private var model
    @State private var rails: [Rail]?
    /// Why the rails could not be read, shown in their place.
    @State private var loadError: String?
    /// Taken off by the viewer this visit, kept off while the core catches up.
    @State private var hidden: Set<String> = []
    @State private var askedForSignals = false
    @State private var ai: Rail?
    /// Asking before anything goes to Gemini (`AIConsentCard`): until it is answered.
    @State private var offerAI = false
    @State private var movie: Movie?
    @State private var series: Int64?
    /// The billboard's six, kept for the visit (Windows' `decidedOnOpen`).
    @State private var hero: [CatalogItem]?
    #if !os(tvOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @Environment(\.openAssistant) private var openAssistant

    var body: some View {
        NavigationStack {
            content
                .auroraTitle("Home")
                #if os(iOS)
                // Windows' Home has no heading of its own: the top bar names it.
                .toolbarVisibility(.hidden, for: .navigationBar)
                #endif
                .task(id: ReloadKey(library: model.libraryVersion, profile: model.profileId,
                                    showing: movie == nil && series == nil)) {
                    // Rebuilt when a title's page closes: Continue Watching and My List
                    // change from there.
                    guard movie == nil, series == nil else { return }
                    await load()
                }
                .task(id: AiKey(library: model.libraryVersion, profile: model.profileId,
                                version: model.aiVersion)) {
                    let status = await model.geminiStatus()
                    offerAI = !AIConsentCard.wasAnswered && status.map { !$0.enabled } == true
                    ai = await model.aiPicks().flatMap(Self.rail)
                }
                .sheet(item: $movie) { MovieDetail(movie: $0).auroraAppearance() }
                .navigationDestination(item: $series) { SeriesDetail(seriesId: $0) }
                .task(id: model.titleToOpen) { await openLinked() }
        }
    }

    /// A title the Top Shelf opened the app on.
    private func openLinked() async {
        guard let link = model.titleToOpen else { return }
        model.titleToOpen = nil
        switch link.kind {
        case .movie: movie = await model.movieDetail(id: link.id)
        case .series: series = link.id
        }
    }

    @ViewBuilder private var content: some View {
        if let loadError {
            DaylightEmpty(icon: "info.circle", title: "Couldn't load your library", message: loadError)
                .frame(maxHeight: .infinity)
        } else if let rails {
            let shown = visible(rails, ai: ai)
            if shown.isEmpty {
                DaylightEmpty(icon: "house", title: "Nothing here yet",
                              message: "Add a provider in Settings and Aurora will build your library.")
                    .frame(maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: railSpacing) {
                        bento(rails)
                            // In line with the rails' headings.
                            .padding(.horizontal)
                        if offerAI {
                            AIConsentCard { offerAI = false }
                                .padding(.horizontal)
                        }
                        ForEach(shown, id: \.id) { rail in
                            RailRow(rail: rail, hidden: hidden, open: open,
                                    dismiss: dismissAction(for: rail),
                                    ask: rail.kind == "aiPicks" ? { openAssistant(Self.askAbout(rail.items)) } : nil)
                        }
                    }
                    .padding(.vertical)
                }
            }
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// The billboard and "On now": side by side, seven columns to five, where there is
    /// the width (Windows' bento); one above the other on an iPhone.
    @ViewBuilder private func bento(_ rails: [Rail]) -> some View {
        let featured = hero ?? HeroBillboard.pick(from: rails)
        let onNow = OnNowTile(reload: AnyHashable(AiKey(library: model.libraryVersion,
                                                        profile: model.profileId, version: 0)))
        if featured.isEmpty {
            onNow.frame(maxWidth: onNowWidth, alignment: .leading)
        } else if compact {
            VStack(spacing: 16) {
                HeroBillboard(items: featured, open: open)
                onNow
            }
        } else {
            BentoRow(spacing: 16) {
                HeroBillboard(items: featured, open: open)
                onNow
            }
        }
    }

    private var compact: Bool {
        #if os(tvOS)
        false
        #else
        sizeClass == .compact
        #endif
    }

    /// Continue Watching first, then the Gemini rail, then the feed's order; rails
    /// emptied by the viewer go.
    private func visible(_ rails: [Rail], ai: Rail?) -> [Rail] {
        let kept = rails.filter { rail in rail.items.contains { !hidden.contains($0.item.key) } }
        return kept.filter { $0.kind == "continueWatching" } + (ai.map { [$0] } ?? [])
            + kept.filter { $0.kind != "continueWatching" }
    }

    /// The picks as a rail. Named for what it is: "Recommended for you" is the local
    /// recommender's rail, and two rails with one name look confused about their source.
    private static func rail(_ picks: AiPicks) -> Rail? {
        guard !picks.items.isEmpty else { return nil }
        return Rail(id: "gemini", kind: "aiPicks", title: "Because of what you watch",
                    items: picks.items, total: UInt32(picks.items.count), hasMore: false)
    }

    private func load() async {
        let built: [Rail]
        do {
            built = try await model.homeRailsOrFailure()
        } catch is CancellationError {
            return
        } catch {
            // In place before there is anything to show; a failed rebuild keeps the
            // screen it has and says so as every other failure does.
            if rails == nil { loadError = error.viewerMessage } else { model.error = error.viewerMessage }
            return
        }
        loadError = nil
        rails = built
        #if os(tvOS)
        TopShelf.publish(built)
        #endif
        // Chosen once, when the screen first has rails: adding a title to My List from
        // the billboard rebuilds the rails, and must not swap the title being looked at.
        if hero == nil, !built.isEmpty { hero = HeroBillboard.pick(from: built) }
        hidden = []
        // Once a visit, as Windows does: fetch what the feed reads from TMDB, and build
        // again if anything new came back.
        guard !askedForSignals, !built.isEmpty else { return }
        askedForSignals = true
        if await model.refreshHomeSignals() > 0 {
            rails = await model.homeRails()
        }
    }

    private func open(_ item: CatalogItem) {
        switch item {
        case .movie(let m): movie = m
        case .series(let s): series = s.id
        }
    }

    /// What the context menu offers on this rail: nothing on My List (the viewer put it
    /// there), "Remove" on Continue Watching, "Not interested" everywhere else.
    private func dismissAction(for rail: Rail) -> RailRow.Dismiss? {
        switch rail.kind {
        case "myList", "aiPicks": return nil
        case "continueWatching":
            return .init(label: "Remove from Continue Watching", systemImage: "xmark.circle") { item in
                await hide(item) { await model.forgetProgress(item.listKind, id: item.id) != nil }
            }
        default:
            return .init(label: "Not interested", systemImage: "hand.thumbsdown") { item in
                await hide(item) { await model.setNotInterested(item.listKind, id: item.id) != nil }
            }
        }
    }

    /// Hide at once; put it back if the core couldn't do it (the error is already shown).
    private func hide(_ item: CatalogItem, _ action: () async -> Bool) async {
        hidden.insert(item.key)
        if !(await action()) { hidden.remove(item.key) }
    }

    /// The opening question from the rail, as Windows words it: the first four titles,
    /// phrased as something a person would say, since it goes into the thread as theirs.
    static func askAbout(_ items: [RailItem]) -> String {
        let titles = items.prefix(4).map(\.item.title)
        if titles.isEmpty { return "What should I watch tonight?" }
        return "You suggested \(titles.joined(separator: ", ")). Why those, and what else is like them?"
    }

    private struct AiKey: Hashable {
        let library: Int
        let profile: Int64
        let version: Int
    }

    private struct ReloadKey: Equatable {
        let library: Int
        let profile: Int64
        let showing: Bool
    }

    #if os(tvOS)
    private let railSpacing: CGFloat = 48
    /// Windows gives it five of twelve columns beside the billboard.
    private let onNowWidth: CGFloat = 900
    #else
    private let railSpacing: CGFloat = 24
    private let onNowWidth: CGFloat = 640
    #endif
}

/// One rail: its title and a row of cards that pages as it is scrolled.
struct RailRow: View {
    struct Dismiss {
        let label: String
        let systemImage: String
        let action: (CatalogItem) async -> Void
    }

    @Environment(AppModel.self) private var model
    let rail: Rail
    let hidden: Set<String>
    let open: (CatalogItem) -> Void
    let dismiss: Dismiss?
    /// "Ask about these", on the Gemini rail: into the assistant with these titles.
    var ask: (() -> Void)? = nil
    @State private var more: [RailItem] = []
    @State private var hasMore: Bool?
    @State private var loading = false

    private var items: [RailItem] {
        var seen = Set<String>()
        return (rail.items + more).filter { !hidden.contains($0.item.key) && seen.insert($0.item.key).inserted }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(rail.title).displayFont(.title3)
                Spacer()
                if let ask {
                    Button("Ask about these", systemImage: "sparkles", action: ask)
                        .font(.ui(.caption).weight(.semibold))
                        #if os(iOS)
                        .buttonStyle(.borderless)
                        #endif
                }
            }
            .padding(.horizontal)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: cardSpacing) {
                    ForEach(items, id: \.item.key) { entry in
                        Button { open(entry.item) } label: { RailCard(entry: entry, width: cardWidth) }
                            .tileButtonStyle()
                            .contextMenu {
                                if let dismiss {
                                    Button(dismiss.label, systemImage: dismiss.systemImage) {
                                        Task { await dismiss.action(entry.item) }
                                    }
                                }
                            }
                            .onAppear {
                                if entry.item.key == items.last?.item.key { Task { await loadMore() } }
                            }
                    }
                }
                .padding(.horizontal)
                #if os(tvOS)
                .padding(.vertical, 24) // room for the focused card to lift
                #endif
            }
            .scrollClipDisabled()
        }
        #if os(tvOS)
        .focusSection()
        #endif
        .onChange(of: rail.items.map(\.item.key)) { more = []; hasMore = nil }
    }

    private func loadMore() async {
        guard !loading, hasMore ?? rail.hasMore else { return }
        loading = true
        defer { loading = false }
        let offset = rail.items.count + more.count
        guard let page = await model.railPage(rail.id, offset: offset) else { return }
        more += page.items
        hasMore = page.hasMore && !page.items.isEmpty
    }

    #if os(tvOS)
    private let cardWidth: CGFloat = 220
    private let cardSpacing: CGFloat = 40
    #else
    private let cardWidth: CGFloat = 110
    private let cardSpacing: CGFloat = 12
    #endif
}

/// A poster with the rail's line under it, and how far in on Continue Watching.
struct RailCard: View {
    let entry: RailItem
    let width: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            AsyncImage(url: entry.item.poster.flatMap(URL.init(string:))) { image in
                image.resizable().scaledToFill()
            } placeholder: {
                ZStack {
                    Rectangle().fill(.quaternary)
                    Text(entry.item.title).font(.ui(.caption)).multilineTextAlignment(.center).padding(6)
                }
            }
            .frame(width: width, height: width * 3 / 2)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(alignment: .bottom) {
                if let p = entry.progress, p.durationSecs > 0 {
                    ProgressView(value: min(Double(p.positionSecs) / Double(p.durationSecs), 1))
                        .tint(.accentColor)
                        .padding(6)
                }
            }
            Text(entry.item.title).font(.ui(.caption)).lineLimit(1)
            if let reason = entry.reason {
                Text(reason).font(.ui(.caption2)).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .frame(width: width)
    }
}

extension CatalogItem {
    /// `movie:12` / `series:7`, as Windows keys cards.
    var key: String {
        switch self {
        case .movie(let m): "movie:\(m.id)"
        case .series(let s): "series:\(s.id)"
        }
    }

    var id: Int64 {
        switch self {
        case .movie(let m): m.id
        case .series(let s): s.id
        }
    }

    var title: String {
        switch self {
        case .movie(let m): m.title
        case .series(let s): s.title
        }
    }

    var poster: String? {
        switch self {
        case .movie(let m): m.poster
        case .series(let s): s.poster
        }
    }

    var listKind: ListKind {
        switch self {
        case .movie: .movie
        case .series: .series
        }
    }
}
