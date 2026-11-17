import SwiftUI

// MARK: Films

struct MoviesView: View {
    @Environment(AppModel.self) private var model
    @State private var shelves: [Shelf] = []
    @State private var shelf: String?
    @State private var movies: [Movie] = []
    @State private var search = ""
    @State private var selected: Movie?
    @State private var myList = false

    var body: some View {
        NavigationStack {
            ScrollView {
                ShelfPicker(shelves: shelves, selection: $shelf, myList: $myList)
                PosterGrid(items: movies.map { PosterItem(id: $0.id, title: $0.title, year: $0.year, poster: $0.poster) }) { id in
                    selected = movies.first { $0.id == id }
                }
                if movies.isEmpty && myList {
                    ContentUnavailableView("Nothing on My List", systemImage: "plus",
                                           description: Text("Add a film from its page."))
                } else if movies.isEmpty {
                    ContentUnavailableView("No films", systemImage: "film",
                                           description: Text("Import films from a provider in Live TV → Add provider."))
                }
            }
            .navigationTitle("Films")
            .searchable(text: $search)
            .task(id: "\(myList)|\(shelf ?? "")|\(search)|\(selected == nil)|\(model.libraryVersion)") {
                // Reloaded when a film's page closes, so My List shows what was just added.
                guard selected == nil else { return }
                movies = myList
                    ? await model.myList()?.movies ?? []
                    : await model.movies(.shelf(shelf, query: search))
            }
            .task(id: model.libraryVersion) { shelves = await model.movieShelves() }
            .sheet(item: $selected) { MovieDetail(movie: $0) }
        }
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
    @State private var shelves: [Shelf] = []
    @State private var shelf: String?
    @State private var series: [Series] = []
    @State private var search = ""
    @State private var myList = false

    var body: some View {
        NavigationStack {
            ScrollView {
                ShelfPicker(shelves: shelves, selection: $shelf, myList: $myList)
                PosterGrid(items: series.map { PosterItem(id: $0.id, title: $0.title, year: $0.year, poster: $0.poster) },
                           destination: { id in SeriesDetail(seriesId: id) })
                if series.isEmpty && myList {
                    ContentUnavailableView("Nothing on My List", systemImage: "plus",
                                           description: Text("Add a series from its page."))
                } else if series.isEmpty {
                    ContentUnavailableView("No series", systemImage: "rectangle.stack",
                                           description: Text("Import series from a provider in Live TV → Add provider."))
                }
            }
            .navigationTitle("Series")
            .searchable(text: $search)
            .task(id: "\(myList)|\(shelf ?? "")|\(search)|\(model.libraryVersion)") {
                series = myList
                    ? await model.myList()?.series ?? []
                    : await model.series(.shelf(shelf, query: search))
            }
            .onAppear {
                // Back from a series page, which may have added or removed it.
                guard myList else { return }
                Task { series = await model.myList()?.series ?? [] }
            }
            .task(id: model.libraryVersion) { shelves = await model.seriesShelves() }
        }
    }
}

struct SeriesDetail: View {
    @Environment(AppModel.self) private var model
    let seriesId: Int64
    @State private var detail: Series?
    @State private var season: UInt16?
    @State private var episodes: [Episode] = []
    @State private var resume: SeriesResume?
    @State private var playing: PlayerItem?

    var body: some View {
        List {
            if let detail {
                Section {
                    MarkButtons(kind: .series, id: seriesId)
                    if let overview = detail.overview { Text(overview) }
                    if let resume, let episode = episodes.first(where: { $0.id == resume.episodeId }) {
                        Button(resume.positionSecs > 0 ? "Resume \(label(episode))" : "Play \(label(episode))",
                               systemImage: "play.fill") {
                            playing = .episode(id: episode.id, title: label(episode),
                                               startAt: resume.positionSecs > 0 ? Double(resume.positionSecs) : nil)
                        }
                    }
                }
                if detail.seasons.count > 1 {
                    Picker("Season", selection: $season) {
                        ForEach(detail.seasons, id: \.self) { Text("Season \($0)").tag(Optional($0)) }
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
                if episodes.isEmpty {
                    Text("This show's episode list has not been fetched yet.")
                }
            }
        }
        .navigationTitle(detail?.title ?? "")
        .task {
            detail = await loadDetail()
            season = detail?.seasons.first
        }
        .task(id: season) { episodes = await model.episodes(seriesId: seriesId, season: season) }
        .task(id: playing == nil) { resume = await model.seriesResume(seriesId: seriesId) }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
    }

    private func loadDetail() async -> Series? {
        await model.seriesDetail(seriesId: seriesId)
    }

    private func label(_ e: Episode) -> String {
        let number = "S\(e.season)E\(e.episode)"
        guard let title = e.title, !title.isEmpty else { return number }
        return "\(number) · \(title)"
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
        Button(title, action: action)
            .buttonStyle(.bordered)
            .tint(selected ? .accentColor : .secondary)
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

    init(items: [PosterItem], onSelect: @escaping (Int64) -> Void) where Destination == EmptyView {
        self.items = items
        self.onSelect = onSelect
        self.destination = nil
    }

    init(items: [PosterItem], destination: @escaping (Int64) -> Destination) {
        self.items = items
        self.onSelect = nil
        self.destination = destination
    }

    #if os(tvOS)
    private let width: CGFloat = 220
    #else
    private let width: CGFloat = 110
    #endif

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: width), spacing: 16)], spacing: 24) {
            ForEach(items) { item in
                if let destination {
                    NavigationLink { destination(item.id) } label: { PosterCard(item: item) }
                        .tileButtonStyle()
                } else {
                    Button { onSelect?(item.id) } label: { PosterCard(item: item) }
                        .tileButtonStyle()
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
