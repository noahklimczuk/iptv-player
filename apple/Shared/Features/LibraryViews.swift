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
                    ContentUnavailableView {
                        Label("Nothing matches those filters", systemImage: "line.3.horizontal.decrease")
                    } actions: {
                        Button("Clear filters") { filters.clear() }
                    }
                } else if movies.isEmpty, let why = model.unratedHiddenReason {
                    ContentUnavailableView("Nothing rated for this profile yet", systemImage: "lock",
                                           description: Text(why))
                } else if movies.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: "film")
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

/// A film's page, as a sheet: Windows' detail modal (`TitleDetail`).
struct MovieDetail: View {
    let movie: Movie
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        TitleDetail(item: .movie(movie: movie)) { dismiss() }
    }
}

// MARK: Series

struct SeriesView: View {
    @Environment(AppModel.self) private var model
    /// A–Z to start, as Windows opens its series page.
    @State private var filters = BrowseFilters(sort: .title)
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
                    ContentUnavailableView {
                        Label("Nothing matches those filters", systemImage: "line.3.horizontal.decrease")
                    } actions: {
                        Button("Clear filters") { filters.clear() }
                    }
                } else if series.isEmpty, let why = model.unratedHiddenReason {
                    ContentUnavailableView("Nothing rated for this profile yet", systemImage: "lock",
                                           description: Text(why))
                } else if series.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: "rectangle.stack")
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

    /// Windows' "Clear filters": genre, shelf, letter and search; the sort stays.
    mutating func clear() {
        genre = nil
        shelf = nil
        letter = nil
        search = ""
    }
}

/// The browse filters Windows' film and series pages have, in its order: the sort's
/// segments, the commonest genres as chips, the whole genre list and the total, then the
/// letters, which only mean something against A–Z.
struct FilterBar: View {
    @Binding var filters: BrowseFilters
    let facets: BrowseFacets?
    let noun: (one: String, many: String)

    /// Windows' `CHIP_GENRES`.
    private static let chipGenres = 7

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Segments(options: BrowseSort.all.map { ($0, $0.label) }, selection: $filters.sort)
                .accessibilityLabel("Sort")
            if let genres = facets?.genres, !genres.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        FilterChip(title: "All", selected: filters.genre == nil) { filters.genre = nil }
                        ForEach(genres.prefix(Self.chipGenres), id: \.name) { genre in
                            FilterChip(title: genre.name, selected: filters.genre == genre.name) {
                                filters.genre = filters.genre == genre.name ? nil : genre.name
                            }
                        }
                    }
                }
                .accessibilityLabel("Popular genres")
            }
            HStack(spacing: 16) {
                // Only with genres to offer: an "All genres" menu with nothing in it looks
                // broken, and is, on a library its provider sends no genres for.
                if !(facets?.genres.isEmpty ?? true) { genreMenu }
                Spacer(minLength: 16)
                total
            }
            #if os(tvOS)
            // The menu sits at the left, so moving up from a letter on the right would
            // otherwise find nothing above it and jump past it to the chips.
            .focusSection()
            #endif
            LetterBar(selection: $filters.letter)
                // Picking "W" while the list is newest first would quietly hide most of
                // the W titles from view: only against A–Z, as on Windows.
                .disabled(filters.sort != .title)
                .opacity(filters.sort == .title ? 1 : 0.4)
        }
        .buttonStyle(.bordered)
        .padding(.horizontal)
        .onChange(of: filters.sort) { _, sort in
            if sort != .title { filters.letter = nil }
        }
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

/// A show's page, pushed: Windows' detail modal (`TitleDetail`), once the show is read.
struct SeriesDetail: View {
    @Environment(AppModel.self) private var model
    let seriesId: Int64
    @State private var detail: Series?

    var body: some View {
        Group {
            if let detail {
                TitleDetail(item: .series(series: detail))
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        // The page's own heading is the title, large, in the hero.
        .auroraPageTitle("")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .task { detail = await model.seriesDetail(seriesId: seriesId) }
    }
}

// MARK: Shared pieces

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
/// Windows' `dl-seg`: a few choices in a well, the chosen one raised, each as wide as
/// its words (a system segmented control makes them equal, and cut "Recently added" to
/// "Recently ad…"). On a TV, chips, which the focus moves along.
struct Segments<Value: Hashable>: View {
    let options: [(Value, String)]
    @Binding var selection: Value
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        #if os(tvOS)
        HStack(spacing: 8) {
            ForEach(options, id: \.0) { value, label in
                FilterChip(title: label, selected: value == selection) { selection = value }
            }
        }
        .buttonStyle(.bordered)
        #else
        HStack(spacing: 2) {
            ForEach(options, id: \.0) { value, label in
                let active = value == selection
                Button { selection = value } label: {
                    Text(label)
                        .font(.subheadline.weight(.bold))
                        .lineLimit(1)
                        .foregroundStyle(active ? (theme == .contrast ? Color.black : Color.primary) : Color.secondary)
                        .padding(.horizontal, 14)
                        .frame(height: 32)
                        .background {
                            if active {
                                RoundedRectangle(cornerRadius: 8)
                                    .fill(theme == .contrast ? theme.accent : theme.surface)
                                    .shadow(color: .black.opacity(0.08), radius: 1, y: 1)
                            }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(active ? .isSelected : [])
            }
        }
        .padding(4)
        .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: 12))
        .fixedSize()
        #endif
    }
}

/// Windows' `dl-seg` holding actions rather than a choice (the guide's −24h … +24h):
/// the same well, with `active` raised.
struct ActionSegments: View {
    struct Item {
        let title: String
        var systemImage: String?
        var active = false
        let action: () -> Void
    }

    let items: [Item]
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        #if os(tvOS)
        HStack(spacing: 8) {
            ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                Button(action: item.action) { label(item) }
            }
        }
        .buttonStyle(.bordered)
        #else
        HStack(spacing: 2) {
            ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                Button(action: item.action) {
                    label(item)
                        .font(.subheadline.weight(.bold))
                        .foregroundStyle(item.active ? (theme == .contrast ? Color.black : Color.primary) : Color.secondary)
                        .padding(.horizontal, 12)
                        .frame(height: 32)
                        .background {
                            if item.active {
                                RoundedRectangle(cornerRadius: 8)
                                    .fill(theme == .contrast ? theme.accent : theme.surface)
                                    .shadow(color: .black.opacity(0.08), radius: 1, y: 1)
                            }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(4)
        .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: 12))
        .fixedSize()
        #endif
    }

    @ViewBuilder private func label(_ item: Item) -> some View {
        HStack(spacing: 6) {
            if let image = item.systemImage { Image(systemName: image) }
            Text(item.title).lineLimit(1)
        }
    }
}

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
