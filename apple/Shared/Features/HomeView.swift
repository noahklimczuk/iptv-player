import SwiftUI

/// The home screen, as Windows' `HomePage`: Continue Watching first, then the rails the
/// core's feed builds (My List, For you, Top 10, Because you watched…, the shelves).
///
/// Behaviour matches Windows: each rail pages as it is scrolled, a suggestion can be
/// hidden with "Not interested" (gone at once, put back if the core says no), a title
/// can be taken off Continue Watching, and after the first load the TMDB lists the feed
/// reads are refreshed once, rebuilding the rails if any came back.
///
/// Not here yet, for the look-and-feel pass: the hero billboard, the On Now tile and the
/// assistant's rail.
struct HomeView: View {
    @Environment(AppModel.self) private var model
    @State private var rails: [Rail]?
    /// Taken off by the viewer this visit, kept off while the core catches up.
    @State private var hidden: Set<String> = []
    @State private var askedForSignals = false
    @State private var movie: Movie?
    @State private var series: Int64?

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Home")
                .task(id: ReloadKey(library: model.libraryVersion, profile: model.profileId,
                                    showing: movie == nil && series == nil)) {
                    // Rebuilt when a title's page closes: Continue Watching and My List
                    // change from there.
                    guard movie == nil, series == nil else { return }
                    await load()
                }
                .sheet(item: $movie) { MovieDetail(movie: $0) }
                .navigationDestination(item: $series) { SeriesDetail(seriesId: $0) }
        }
    }

    @ViewBuilder private var content: some View {
        if let rails {
            let shown = visible(rails)
            if shown.isEmpty {
                ContentUnavailableView("Nothing here yet", systemImage: "house",
                                       description: Text("Add a provider in Settings and Aurora will build your library."))
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: railSpacing) {
                        ForEach(shown, id: \.id) { rail in
                            RailRow(rail: rail, hidden: hidden, open: open,
                                    dismiss: dismissAction(for: rail))
                        }
                    }
                    .padding(.vertical)
                }
            }
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// Continue Watching first, then the feed's order; rails emptied by the viewer go.
    private func visible(_ rails: [Rail]) -> [Rail] {
        let kept = rails.filter { rail in rail.items.contains { !hidden.contains($0.item.key) } }
        return kept.filter { $0.kind == "continueWatching" } + kept.filter { $0.kind != "continueWatching" }
    }

    private func load() async {
        let built = await model.homeRails()
        rails = built
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
        case "myList": return nil
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

    private struct ReloadKey: Equatable {
        let library: Int
        let profile: Int64
        let showing: Bool
    }

    #if os(tvOS)
    private let railSpacing: CGFloat = 48
    #else
    private let railSpacing: CGFloat = 24
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
    @State private var more: [RailItem] = []
    @State private var hasMore: Bool?
    @State private var loading = false

    private var items: [RailItem] {
        var seen = Set<String>()
        return (rail.items + more).filter { !hidden.contains($0.item.key) && seen.insert($0.item.key).inserted }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(rail.title).font(.title3.bold()).padding(.horizontal)
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
                    Text(entry.item.title).font(.caption).multilineTextAlignment(.center).padding(6)
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
            Text(entry.item.title).font(.caption).lineLimit(1)
            if let reason = entry.reason {
                Text(reason).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
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
