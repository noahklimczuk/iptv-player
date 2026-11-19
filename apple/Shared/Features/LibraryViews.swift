import SwiftUI

// MARK: Films

struct MoviesView: View {
    @Environment(AppModel.self) private var model
    @State private var filters = BrowseFilters()
    @State private var facets: BrowseFacets?
    @State private var movies: [Movie] = []
    /// The first page arrived: until then empty means "not read yet", not "none".
    @State private var loaded = false
    @State private var selected: Movie?

    var body: some View {
        NavigationStack {
            ScrollView {
                // The shelf, where one is chosen, as Windows' heading names it.
                PageHeading(filters.shelf ?? "Movies")
                ShelfPicker(shelves: facets?.categories ?? [], selection: $filters.shelf, myList: $filters.myList)
                if !filters.myList {
                    FilterBar(filters: $filters, facets: facets, noun: ("film", "films"))
                }
                PosterGrid(items: movies.map { PosterItem(id: $0.id, title: $0.title, year: $0.year, poster: $0.poster) },
                           onSelect: { id in selected = movies.first { $0.id == id } },
                           onReachEnd: { Task { await loadMore() } })
                if movies.isEmpty && !loaded {
                    ProgressView().padding(.top, 40)
                } else if movies.isEmpty && filters.myList {
                    ContentUnavailableView("Nothing on My List", systemImage: "plus",
                                           description: Text("Add a film from its page."))
                } else if movies.isEmpty && filters.narrowed {
                    ContentUnavailableView("No films match", systemImage: "line.3.horizontal.decrease",
                                           description: Text("Try another letter, genre or shelf."))
                } else if movies.isEmpty, let why = model.unratedHiddenReason {
                    ContentUnavailableView("Nothing rated for this profile yet", systemImage: "lock",
                                           description: Text(why))
                } else if movies.isEmpty {
                    ContentUnavailableView("No films", systemImage: "film",
                                           description: Text("Import films from a provider in Live TV → Add provider."))
                }
            }
            .auroraTitle("Movies")
            .searchable(text: $filters.search)
            .task(id: ReloadKey(filters: filters, library: model.libraryVersion, extra: selected == nil)) {
                // Reloaded when a film's page closes, so My List shows what was just added.
                guard selected == nil else { return }
                await reload()
            }
            .sheet(item: $selected) { MovieDetail(movie: $0).auroraAppearance() }
        }
    }

    private func reload() async {
        let asked = filters
        if asked.myList {
            movies = await model.myList()?.movies ?? []
            loaded = true
            if facets == nil { facets = await model.movieFacets(BrowseFilters().browse()) }
            return
        }
        async let page = model.movies(asked.browse())
        async let counted = model.movieFacets(asked.browse())
        let (rows, f) = await (page, counted)
        guard asked == filters else { return }
        movies = rows
        facets = f
        loaded = true
    }

    /// The next page, when the last poster comes on screen and there is more.
    private func loadMore() async {
        let asked = filters
        guard !asked.myList, let total = facets?.total, movies.count < Int(total) else { return }
        let more = await model.movies(asked.browse(offset: UInt32(movies.count)))
        // The filters may have changed while this was on its way: then it's another list.
        guard asked == filters else { return }
        let known = Set(movies.map(\.id))
        movies += more.filter { !known.contains($0.id) }
    }
}

struct MovieDetail: View {
    @Environment(AppModel.self) private var model
    let movie: Movie
    @State private var progress: WatchProgress?
    @State private var playing: PlayerItem?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(movie.title).font(.largeTitle.bold())
                    HStack {
                        if let year = movie.year { Text(String(year)) }
                        if let mins = movie.runtimeMins { Text("\(mins) min") }
                        if let rating = movie.rating { Text(String(format: "★ %.1f", rating)) }
                    }
                    .foregroundStyle(.secondary)
                    if let overview = movie.overview { Text(overview) }
                    CastAndCrew(kind: .movie, id: movie.id, genres: movie.genres)
                    PlayButtons(progress: progress) { startAt in
                        playing = .movie(id: movie.id, title: movie.title, startAt: startAt)
                    }
                    MarkButtons(kind: .movie, id: movie.id)
                }
                .padding()
            }
            .task(id: playing == nil) { progress = await model.progress(.movie, id: movie.id) }
            .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
        }
    }
}

// MARK: Series

struct SeriesView: View {
    @Environment(AppModel.self) private var model
    @State private var filters = BrowseFilters()
    @State private var facets: BrowseFacets?
    @State private var series: [Series] = []
    /// The first page arrived: until then empty means "not read yet", not "none".
    @State private var loaded = false

    var body: some View {
        NavigationStack {
            ScrollView {
                PageHeading(filters.shelf ?? "Series")
                ShelfPicker(shelves: facets?.categories ?? [], selection: $filters.shelf, myList: $filters.myList)
                if !filters.myList {
                    FilterBar(filters: $filters, facets: facets, noun: ("series", "series"))
                }
                PosterGrid(items: series.map { PosterItem(id: $0.id, title: $0.title, year: $0.year, poster: $0.poster) },
                           destination: { id in SeriesDetail(seriesId: id) },
                           onReachEnd: { Task { await loadMore() } })
                if series.isEmpty && !loaded {
                    ProgressView().padding(.top, 40)
                } else if series.isEmpty && filters.myList {
                    ContentUnavailableView("Nothing on My List", systemImage: "plus",
                                           description: Text("Add a series from its page."))
                } else if series.isEmpty && filters.narrowed {
                    ContentUnavailableView("No series match", systemImage: "line.3.horizontal.decrease",
                                           description: Text("Try another letter, genre or shelf."))
                } else if series.isEmpty, let why = model.unratedHiddenReason {
                    ContentUnavailableView("Nothing rated for this profile yet", systemImage: "lock",
                                           description: Text(why))
                } else if series.isEmpty {
                    ContentUnavailableView("No series", systemImage: "rectangle.stack",
                                           description: Text("Import series from a provider in Live TV → Add provider."))
                }
            }
            .auroraTitle("Series")
            .searchable(text: $filters.search)
            .task(id: ReloadKey(filters: filters, library: model.libraryVersion)) { await reload() }
            .onAppear {
                // Back from a series page, which may have added or removed it.
                guard filters.myList else { return }
                Task { series = await model.myList()?.series ?? [] }
            }
        }
    }

    private func reload() async {
        let asked = filters
        if asked.myList {
            series = await model.myList()?.series ?? []
            loaded = true
            if facets == nil { facets = await model.seriesFacets(BrowseFilters().browse()) }
            return
        }
        async let page = model.series(asked.browse())
        async let counted = model.seriesFacets(asked.browse())
        let (rows, f) = await (page, counted)
        guard asked == filters else { return }
        series = rows
        facets = f
        loaded = true
    }

    private func loadMore() async {
        let asked = filters
        guard !asked.myList, let total = facets?.total, series.count < Int(total) else { return }
        let more = await model.series(asked.browse(offset: UInt32(series.count)))
        guard asked == filters else { return }
        let known = Set(series.map(\.id))
        series += more.filter { !known.contains($0.id) }
    }
}

/// What a browse screen reloads on: its filters, the library, and one thing of its own.
private struct ReloadKey: Equatable {
    var filters: BrowseFilters
    var library: Int
    var extra = false
}

extension BrowseFilters {
    /// Narrowed by more than the shelf: an empty list then means "nothing matches",
    /// not "nothing imported".
    var narrowed: Bool { shelf != nil || genre != nil || letter != nil || !search.isEmpty }
}

/// Sort, genre and the total on one row, the letters under it: the browse filters
/// Windows' film and series pages have.
struct FilterBar: View {
    @Binding var filters: BrowseFilters
    let facets: BrowseFacets?
    let noun: (one: String, many: String)

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Sort, genre and the total on one line where they fit (a TV, an iPad); on a
            // phone the total goes underneath rather than squeezing the genre to "A…".
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 16) {
                    sortMenu
                    genreMenu
                    Spacer(minLength: 16)
                    total
                }
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 12) {
                        sortMenu
                        genreMenu
                    }
                    total
                }
            }
            #if os(tvOS)
            // The two menus sit at the left, so moving up from a letter on the right
            // would otherwise find nothing above it and jump past them to the shelves.
            .focusSection()
            #endif
            LetterBar(selection: $filters.letter)
        }
        .buttonStyle(.bordered)
        .padding(.horizontal)
    }

    private var sortMenu: some View {
        Menu {
            ForEach(BrowseSort.all, id: \.self) { sort in
                Button {
                    filters.sort = sort
                } label: {
                    if filters.sort == sort { Label(sort.label, systemImage: "checkmark") } else { Text(sort.label) }
                }
            }
        } label: {
            Label(filters.sort.label, systemImage: "arrow.up.arrow.down")
                .lineLimit(1)
        }
        .fixedSize()
    }

    private var genreMenu: some View {
        Menu {
            Button("All genres") { filters.genre = nil }
            // Commonest first, with how many: on a real panel most of a few hundred
            // genres hold two titles or fewer.
            ForEach(facets?.genres ?? [], id: \.name) { genre in
                Button {
                    filters.genre = genre.name
                } label: {
                    if filters.genre == genre.name {
                        Label("\(genre.name) (\(genre.count))", systemImage: "checkmark")
                    } else {
                        Text("\(genre.name) (\(genre.count))")
                    }
                }
            }
        } label: {
            // One line: a long genre name is cut short, not wrapped.
            Label(filters.genre ?? "All genres", systemImage: "theatermasks")
                .lineLimit(1)
        }
        // A library without TMDB data has shelves but no genres.
        .disabled(facets?.genres.isEmpty ?? true)
    }

    @ViewBuilder
    private var total: some View {
        if let total = facets?.total {
            Text("\(total.formatted()) \(total == 1 ? noun.one : noun.many)")
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

/// A–Z, and `#` for titles starting with anything else. A filter, not a jump: the list
/// is paged, so "jump to W" can't be answered without reading every page before it.
struct LetterBar: View {
    @Binding var selection: String?
    private static let letters = ["#"] + (UnicodeScalar("A").value...UnicodeScalar("Z").value)
        .compactMap { UnicodeScalar($0).map { String($0) } }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                FilterChip(title: "All", selected: selection == nil) { selection = nil }
                ForEach(Self.letters, id: \.self) { letter in
                    FilterChip(title: letter, selected: selection == letter) { selection = letter }
                }
            }
        }
    }
}

struct SeriesDetail: View {
    @Environment(AppModel.self) private var model
    let seriesId: Int64
    @State private var detail: Series?
    @State private var season: UInt16?
    /// Every episode of every season, every copy of the show merged (the core does that).
    /// The season picker comes from these, not from the show's row, as on Windows: a
    /// season only a hidden duplicate has is still reachable.
    @State private var allEpisodes: [Episode] = []
    @State private var loading = true
    @State private var resume: SeriesResume?
    @State private var playing: PlayerItem?

    private var seasons: [UInt16] { Array(Set(allEpisodes.map(\.season))).sorted() }
    private var episodes: [Episode] { allEpisodes.filter { $0.season == season } }

    var body: some View {
        List {
            if let detail {
                Section {
                    MarkButtons(kind: .series, id: seriesId)
                    if let overview = detail.overview { Text(overview) }
                    CastAndCrew(kind: .series, id: seriesId, genres: detail.genres)
                    SeriesSwitches(seriesId: seriesId)
                    if let resume, let episode = allEpisodes.first(where: { $0.id == resume.episodeId }) {
                        Button(resume.positionSecs > 0 ? "Resume \(label(episode))" : "Play \(label(episode))",
                               systemImage: "play.fill") {
                            playing = .episode(id: episode.id, title: label(episode),
                                               startAt: resume.positionSecs > 0 ? Double(resume.positionSecs) : nil)
                        }
                    }
                }
                if seasons.count > 1 {
                    Picker("Season", selection: $season) {
                        ForEach(seasons, id: \.self) { Text("Season \($0)").tag(Optional($0)) }
                    }
                }
            }
            Section {
                ForEach(episodes, id: \.id) { episode in
                    Button {
                        playing = .episode(id: episode.id, title: label(episode), startAt: nil)
                    } label: {
                        VStack(alignment: .leading) {
                            Text(label(episode))
                            if let overview = episode.overview {
                                Text(overview).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                            }
                        }
                    }
                }
            } footer: {
                if loading && allEpisodes.isEmpty {
                    // The first time a show is opened its listing comes from the provider.
                    ProgressView("Fetching episodes…")
                } else if allEpisodes.isEmpty {
                    Text("The provider doesn't list any episodes for this show yet.")
                }
            }
        }
        .auroraPageTitle(detail?.title ?? "")
        .task {
            detail = await model.seriesDetail(seriesId: seriesId)
            allEpisodes = await model.episodes(seriesId: seriesId, season: nil)
            loading = false
            // Where the viewer is, or the first season.
            let resumeSeason = allEpisodes.first { $0.id == resume?.episodeId }?.season
            season = resumeSeason ?? seasons.first
        }
        .task(id: playing == nil) { resume = await model.seriesResume(seriesId: seriesId) }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
    }

    private func label(_ e: Episode) -> String {
        let number = "S\(e.season)E\(e.episode)"
        guard let title = e.title, !title.isEmpty else { return number }
        return "\(number) · \(title)"
    }
}

/// The show's own switches, read by the player's skip and Up Next (`DetailModal.tsx`
/// `SeriesPrefsToggles`). Per profile.
private struct SeriesSwitches: View {
    @Environment(AppModel.self) private var model
    let seriesId: Int64
    @State private var prefs: SeriesPrefs?

    var body: some View {
        if let prefs {
            toggle("Always skip intros", \.alwaysSkipIntro, prefs)
            toggle("Always skip recaps", \.alwaysSkipRecap, prefs)
            toggle("Autoplay next episode", \.autoplayNext, prefs)
        } else {
            Color.clear.frame(height: 0)
                .task(id: model.profileId) {
                    prefs = try? model.core?.seriesPrefs(profileId: model.profileId, seriesId: seriesId)
                }
        }
    }

    private func toggle(_ label: String, _ key: WritableKeyPath<SeriesPrefs, Bool>, _ current: SeriesPrefs) -> some View {
        Toggle(label, isOn: Binding(get: { current[keyPath: key] }, set: { on in
            var next = current
            next[keyPath: key] = on
            prefs = next
            do {
                try model.core?.setSeriesPrefs(profileId: model.profileId, seriesId: seriesId, prefs: next)
            } catch {
                // Put it back: a switch showing what wasn't saved would have the player
                // skip (or not) against what the viewer sees (R-103).
                prefs = current
                model.error = "Could not save that preference: \(error.viewerMessage)"
            }
        }))
    }
}

// MARK: Shared pieces

/// Resume / Start over, or just Play.
struct PlayButtons: View {
    let progress: WatchProgress?
    let play: (Double?) -> Void

    var body: some View {
        HStack {
            // Past a minute and not finished is worth returning to; the same threshold
            // the shared resume rule uses (`progress::resume_point`).
            if let p = progress, !p.completed, p.positionSecs > 60 {
                Button("Resume from \(clock(p.positionSecs))", systemImage: "play.fill") { play(Double(p.positionSecs)) }
                Button("Start over", systemImage: "arrow.counterclockwise") { play(nil) }
            } else {
                Button("Play", systemImage: "play.fill") { play(nil) }
            }
        }
        .buttonStyle(.borderedProminent)
    }

    private func clock(_ secs: Int64) -> String {
        let h = secs / 3600, m = (secs % 3600) / 60, s = secs % 60
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
    }
}

struct ShelfPicker: View {
    let shelves: [Shelf]
    @Binding var selection: String?
    /// The profile's My List instead of a shelf.
    @Binding var myList: Bool

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack {
                chip("My List", selected: myList) { myList = true }
                chip("All", selected: !myList && selection == nil) { myList = false; selection = nil }
                ForEach(shelves, id: \.name) { shelf in
                    chip(shelf.name, selected: !myList && selection == shelf.name) {
                        myList = false
                        selection = shelf.name
                    }
                }
            }
            .padding(.horizontal)
        }
    }

    private func chip(_ title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        FilterChip(title: title, selected: selected, action: action)
    }
}

/// One choice in a row of them, tinted when chosen.
struct FilterChip: View {
    let title: String
    let selected: Bool
    /// How many it holds, after the title (the guide's group chips).
    var count: Int64? = nil
    /// Chosen to be left out, so struck through rather than lit (the guide's Hide mode).
    var struck = false
    let action: () -> Void

    var body: some View {
        // tvOS draws a bordered button the same whatever its tint, so the choice shows in
        // the label's weight too. One button either way: swapping styles on selection would
        // replace the focused view and throw the focus off it.
        Button(action: action) {
            // Weight, not colour: a colour set here would also stay on the white focus
            // background, where tvOS otherwise turns the label dark.
            HStack(spacing: 6) {
                Text(title).fontWeight(selected ? .heavy : .regular).strikethrough(selected && struck)
                if let count {
                    Text("\(count)").font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .buttonStyle(.bordered)
        .tint(selected ? .accentColor : .secondary)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// Genres, then the cast and director from TMDB, as Windows' detail page lists them:
/// the first six billed names, nothing where nothing is known.
struct CastAndCrew: View {
    @Environment(AppModel.self) private var model
    let kind: ListKind
    let id: Int64
    let genres: [String]
    @State private var credits: [Credit] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            meta("Genres", genres.joined(separator: ", "))
            meta("Cast", credits.filter(\.isCast).prefix(6).map(\.name).joined(separator: ", "))
            let directors = credits.filter { !$0.isCast && $0.role?.lowercased() == "director" }
            meta(directors.count == 1 ? "Director" : "Directors", directors.map(\.name).joined(separator: ", "))
        }
        .font(.callout)
        // A finished sweep may have just found this title.
        .task(id: model.libraryVersion) { credits = await model.credits(kind, id: id) }
    }

    @ViewBuilder
    private func meta(_ label: String, _ value: String) -> some View {
        if !value.isEmpty {
            (Text("\(label): ").foregroundStyle(.tertiary) + Text(value).foregroundStyle(.secondary))
        }
    }
}

/// My List and Like, for a film's or a series' page.
struct MarkButtons: View {
    @Environment(AppModel.self) private var model
    let kind: ListKind
    let id: Int64
    @State private var marks: ItemMarks?

    var body: some View {
        HStack {
            Button {
                Task { if let now = await model.toggleMyList(kind, id: id) { marks?.inMyList = now } }
            } label: {
                Label("My List", systemImage: marks?.inMyList == true ? "checkmark" : "plus")
            }
            Button {
                Task { if let now = await model.toggleLiked(kind, id: id) { marks?.liked = now } }
            } label: {
                Label("Like", systemImage: marks?.liked == true ? "hand.thumbsup.fill" : "hand.thumbsup")
            }
        }
        .buttonStyle(.bordered)
        .disabled(marks == nil)
        .task { marks = await model.marks(kind, id: id) }
    }
}

struct PosterItem: Identifiable {
    let id: Int64
    let title: String
    let year: Int32?
    let poster: String?
}

/// Posters in a grid that fits the screen: a few columns on a phone, many on a TV.
struct PosterGrid<Destination: View>: View {
    let items: [PosterItem]
    var onSelect: ((Int64) -> Void)?
    var destination: ((Int64) -> Destination)?
    /// The last poster came on screen: time for the next page, if there is one.
    var onReachEnd: (() -> Void)?

    init(items: [PosterItem], onSelect: @escaping (Int64) -> Void,
         onReachEnd: (() -> Void)? = nil) where Destination == EmptyView {
        self.items = items
        self.onSelect = onSelect
        self.destination = nil
        self.onReachEnd = onReachEnd
    }

    init(items: [PosterItem], destination: @escaping (Int64) -> Destination,
         onReachEnd: (() -> Void)? = nil) {
        self.items = items
        self.onSelect = nil
        self.destination = destination
        self.onReachEnd = onReachEnd
    }

    #if os(tvOS)
    private let width: CGFloat = 220
    #else
    private let width: CGFloat = 110
    #endif

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: width), spacing: 16)], spacing: 24) {
            ForEach(items) { item in
                Group {
                    if let destination {
                        NavigationLink { destination(item.id) } label: { PosterCard(item: item) }
                            .tileButtonStyle()
                    } else {
                        Button { onSelect?(item.id) } label: { PosterCard(item: item) }
                            .tileButtonStyle()
                    }
                }
                .onAppear {
                    if item.id == items.last?.id { onReachEnd?() }
                }
            }
        }
        .padding()
    }
}

extension View {
    /// A poster or profile tile: on tvOS the card style, which lifts the focused tile so
    /// the Siri Remote's position can be seen (`.plain` shows none there); plain elsewhere.
    func tileButtonStyle() -> some View {
        #if os(tvOS)
        buttonStyle(.card)
        #else
        buttonStyle(.plain)
        #endif
    }
}

struct PosterCard: View {
    let item: PosterItem

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            AsyncImage(url: item.poster.flatMap(URL.init(string:))) { image in
                image.resizable().scaledToFill()
            } placeholder: {
                ZStack {
                    Rectangle().fill(.quaternary)
                    Text(item.title).font(.caption).multilineTextAlignment(.center).padding(6)
                }
            }
            .aspectRatio(2 / 3, contentMode: .fit)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            Text(item.title).font(.caption).lineLimit(1)
        }
    }
}

extension Movie: Identifiable {}
