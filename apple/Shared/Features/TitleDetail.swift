import SwiftUI

/// A film's or a show's page: Windows' `DetailModal`, one page for both.
///
/// The poster beside everything else; the hero (two genres and the rating as chips, the
/// title, how far into a film the viewer is, Play or Resume, My List and Like); the facts
/// (year, certification, runtime or seasons, quality), the overview and the other copies,
/// with the cast, director, genres and rating beside them; then a well of tabs: a show's
/// episodes, More Like This, and the details.
///
/// Departures (apple/README.md): no Trailer button on an Apple TV, which has no web view
/// for YouTube's embed; on a phone the poster sits above the rest rather than beside it.
struct TitleDetail: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    let item: CatalogItem
    /// Shown as a sheet, so it carries Windows' close button.
    var close: (() -> Void)?

    enum Tab: Hashable {
        case episodes, similar, details
        var label: String {
            switch self {
            case .episodes: "Episodes"
            case .similar: "More Like This"
            case .details: "Details"
            }
        }
    }

    @State private var tab: Tab
    @State private var season: UInt16 = 1
    @State private var progress: WatchProgress?
    @State private var resume: SeriesResume?
    @State private var episodes: [Episode] = []
    @State private var loadingEpisodes: Bool
    @State private var seen: [Int64: EpisodeProgress] = [:]
    @State private var marks: ItemMarks?
    @State private var alternates: [Alternate] = []
    @State private var credits: [Credit] = []
    @State private var similar: [CatalogItem]?
    @State private var playing: PlayerItem?
    /// The trailer, when asked for (Windows' `playingTrailer`). Never by itself here: the
    /// page is opened to read about the title, and a video starting under the text would
    /// interrupt that. With sound and YouTube's controls, as it is what is being watched.
    @State private var playingTrailer = false
    /// The season is placed on the viewer's episode once, not every time it changes.
    @State private var placed = false

    init(item: CatalogItem, close: (() -> Void)? = nil) {
        self.item = item
        self.close = close
        _tab = State(initialValue: item.isSeries ? .episodes : .similar)
        _loadingEpisodes = State(initialValue: item.isSeries)
    }

    var body: some View {
        ScrollView {
            Group {
                if wide {
                    HStack(alignment: .top, spacing: 32) {
                        poster.frame(width: posterWidth)
                        content
                    }
                } else {
                    VStack(alignment: .leading, spacing: 20) {
                        poster.frame(width: 200).frame(maxWidth: .infinity)
                        content
                    }
                    // Above the poster on a phone, where the hero is far down the page.
                    .overlay(alignment: .topTrailing) { closeButton }
                }
            }
            .padding(pagePadding)
            .frame(maxWidth: maxWidth)
            .frame(maxWidth: .infinity)
        }
        .background(theme.surface)
        .task { await loadOnce() }
        // What the household's other devices changed, now rather than at the next round
        // (Windows' `DetailModal`): stop a film on the TV, open it here a moment later,
        // and it offers to resume where the TV stopped. Anything new, and the page reads
        // its progress and marks again.
        .task(id: item.key) {
            guard await model.syncCatchUp(item.listKind, id: item.id) else { return }
            marks = await model.marks(item.listKind, id: item.id)
            await loadProgress()
        }
        // Again after playing: the position, the episode to resume and the ticks moved.
        .task(id: playing == nil) { await loadProgress() }
        // Never a trailer under the player.
        .onChange(of: playing != nil) { _, open in if open { playingTrailer = false } }
        // A finished sweep may have just found this title's cast.
        .task(id: model.libraryVersion) { credits = await model.credits(item.listKind, id: item.id) }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 0) {
            hero
            info.padding(.top, 24)
            Segments(options: tabs.map { ($0, $0.label) }, selection: $tab)
                .padding(.top, 24).padding(.bottom, 16)
            switch tab {
            case .episodes: episodeList
            case .similar: similarGrid
            case .details: details
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var tabs: [Tab] { item.isSeries ? [.episodes, .similar, .details] : [.similar, .details] }

    // MARK: Poster

    private var poster: some View {
        AsyncImage(url: item.poster.flatMap(URL.init(string:))) { image in
            image.resizable().scaledToFill()
        } placeholder: {
            ZStack {
                Rectangle().fill(ChannelTile.hue(item.title))
                Text(item.title)
                    .displayFont(.title3)
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)
                    .padding()
            }
        }
        .aspectRatio(2 / 3, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 24))
        .accessibilityHidden(true)
    }

    // MARK: Hero

    private var hero: some View {
        VStack(alignment: .leading, spacing: 0) {
            #if os(iOS)
            if playingTrailer, let key = item.trailerKey {
                TrailerFrame(key: key, muted: false, controls: true, loop: false)
                    .aspectRatio(16 / 9, contentMode: .fit)
                    .frame(maxWidth: .infinity)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .padding(.bottom, 16)
                // Just a way back, clear of YouTube's own controls along the bottom.
                Button("Stop trailer", systemImage: "xmark") { playingTrailer = false }
                    .buttonStyle(.daylight(.secondary, .lg))
            } else {
                heroText
            }
            #else
            heroText
            #endif
        }
        .padding(.trailing, close != nil && wide ? 64 : 0)
        .overlay(alignment: .topTrailing) {
            if wide { closeButton.offset(x: 8) }
        }
    }

    private var heroText: some View {
        VStack(alignment: .leading, spacing: 0) {
            FlowLayout(spacing: 8) {
                ForEach(Array(item.genres.prefix(2)), id: \.self) { Chip(text: $0) }
                if let rating = item.rating { Chip(text: String(format: "%.1f", rating), accent: true) }
            }
            .padding(.bottom, 14)
            Text(item.title)
                .displayFont(size: titleSize)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.bottom, 16)
                .accessibilityAddTraits(.isHeader)
            if let p = partway {
                VStack(alignment: .leading, spacing: 6) {
                    Meter(fraction: Double(p.positionSecs) / Double(p.durationSecs), height: 4)
                    Text("\(duration(p.positionSecs)) of \(duration(p.durationSecs))")
                        .font(.ui(.caption).weight(.bold))
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: 320)
                .padding(.bottom, 12)
            }
            actions
        }
    }

    @ViewBuilder private var closeButton: some View {
        if let close {
            IconSquare(systemImage: "xmark", label: "Close", size: 44, action: close)
        }
    }

    /// A film started and not finished.
    private var partway: WatchProgress? {
        guard let p = progress, !item.isSeries, !p.completed, p.positionSecs > 0, p.durationSecs > 0 else { return nil }
        return p
    }

    private var actions: some View {
        #if os(tvOS)
        HStack(spacing: 16) { playButton; listButton; likeButton }
        #else
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) { playButton; trailerButton; listButton; likeButton }
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) { playButton; trailerButton }
                HStack(spacing: 8) { listButton; likeButton }
            }
            VStack(alignment: .leading, spacing: 8) {
                playButton
                HStack(spacing: 8) { trailerButton; listButton; likeButton }
            }
        }
        #endif
    }

    private var playButton: some View {
        Button { Task { await play() } } label: {
            Label(playLabel, systemImage: "play.fill")
        }
        #if os(tvOS)
        .buttonStyle(.borderedProminent)
        #else
        .buttonStyle(PrimaryLargeStyle())
        #endif
    }

    /// Windows' words: Resume from where a film was left, the show's episode to go on
    /// with, else Play.
    private var playLabel: String {
        if let p = partway { return "Resume from \(duration(p.positionSecs))" }
        if let resume, let episode = episodes.first(where: { $0.id == resume.episodeId }) {
            return resume.positionSecs > 0
                ? "Resume \(Self.code(episode)) from \(duration(resume.positionSecs))"
                : "Play \(Self.code(episode))"
        }
        return "Play"
    }

    /// Only when there is one: a disabled button would promise what the library cannot
    /// give until the metadata sweep reaches this title.
    @ViewBuilder private var trailerButton: some View {
        #if os(iOS)
        if item.trailerKey != nil {
            Button("Trailer", systemImage: "play") { playingTrailer = true }
                .buttonStyle(.daylight(.secondary, .lg))
        }
        #endif
    }

    private var listButton: some View {
        let on = marks?.inMyList == true
        return IconSquare(systemImage: on ? "checkmark" : "plus",
                          label: on ? "Remove \(item.title) from My List" : "Add \(item.title) to My List",
                          size: 52, active: on) {
            Task { if let now = await model.toggleMyList(item.listKind, id: item.id) { marks?.inMyList = now } }
        }
        .disabled(marks == nil)
    }

    private var likeButton: some View {
        let on = marks?.liked == true
        return IconSquare(systemImage: on ? "hand.thumbsup.fill" : "hand.thumbsup",
                          label: on ? "Undo liking \(item.title)" : "I like \(item.title)",
                          size: 52, active: on) {
            Task { if let now = await model.toggleLiked(item.listKind, id: item.id) { marks?.liked = now } }
        }
        .disabled(marks == nil)
    }

    // MARK: Facts, overview, sources, credits

    private var info: some View {
        let left = VStack(alignment: .leading, spacing: 12) {
            facts
            if let overview = item.overview, !overview.isEmpty {
                Text(overview).lineSpacing(4).fixedSize(horizontal: false, vertical: true)
            }
            sources
        }
        let right = VStack(alignment: .leading, spacing: 12) { credit }
            .font(.ui(.subheadline))
        return Group {
            if wide {
                RatioColumns(ratio: 1.6, spacing: 24) { left; right }
            } else {
                VStack(alignment: .leading, spacing: 20) { left; right }
            }
        }
    }

    private var facts: some View {
        FlowLayout(spacing: 12) {
            if let year = item.year { Text(String(year)).foregroundStyle(.secondary) }
            if let cert = item.certification { Badge(text: cert, style: .outline) }
            if case .movie(let m) = item, !runtime(m.runtimeMins).isEmpty {
                Text(runtime(m.runtimeMins)).foregroundStyle(.secondary)
            }
            if item.isSeries {
                let count = Set(episodes.map(\.season)).count
                Text(loadingEpisodes && episodes.isEmpty
                     ? "Loading episodes…"
                     : "\(count) season\(count == 1 ? "" : "s")")
                    .foregroundStyle(.secondary)
            }
            if let quality = item.quality { Badge(text: quality, style: quality == "4K" ? .accent : .neutral) }
        }
        .font(.ui(.callout).weight(.bold))
    }

    /// Windows' `Sources`: the other providers' copies, each playable.
    @ViewBuilder private var sources: some View {
        let others = alternates.filter { $0.id != item.id }
        if !others.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Text("Also available as")
                    .font(.ui(.caption).weight(.bold))
                    .tracking(0.7)
                    .textCase(.uppercase)
                    .foregroundStyle(.tertiary)
                FlowLayout(spacing: 6) {
                    ForEach(others, id: \.id) { copy in
                        Button { Task { await play(copy: copy.id) } } label: {
                            Label("\(copy.quality ?? "Unknown quality")\(copy.provider.map { " · \($0)" } ?? "")",
                                  systemImage: "play.fill")
                        }
                        #if os(tvOS)
                        .buttonStyle(.bordered)
                        #else
                        .buttonStyle(SecondaryButtonStyle())
                        #endif
                    }
                }
            }
            .padding(.top, 4)
        }
    }

    /// Windows' `CastAndCrew` and `Meta`: TMDB's cast and director, else the provider's
    /// cast; then the genres and the rating.
    @ViewBuilder private var credit: some View {
        let cast = credits.filter(\.isCast)
        if cast.isEmpty {
            meta("Cast", item.cast.joined(separator: ", "))
        } else {
            meta("Cast", cast.prefix(6).map(\.name).joined(separator: ", "))
            let directors = credits.filter { !$0.isCast && $0.role?.lowercased() == "director" }
            meta(directors.count == 1 ? "Director" : "Directors", directors.map(\.name).joined(separator: ", "))
        }
        meta("Genres", item.genres.joined(separator: ", "))
        if let rating = item.rating { meta("Rating", String(format: "%.1f / 10", rating)) }
    }

    @ViewBuilder private func meta(_ label: String, _ value: String) -> some View {
        if !value.isEmpty {
            Text("\(Text("\(label): ").foregroundStyle(.tertiary))\(Text(value).foregroundStyle(.secondary))")
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    // MARK: Episodes

    private var seasons: [UInt16] { Array(Set(episodes.map(\.season))).sorted() }

    private var episodeList: some View {
        let shown = seasons.count > 1 ? episodes.filter { $0.season == season } : episodes
        return VStack(alignment: .leading, spacing: 16) {
            SeriesPrefsChips(seriesId: item.id)
            if seasons.count > 1 {
                SeasonMenu(seasons: seasons, season: $season)
            }
            VStack(spacing: 4) {
                if loadingEpisodes {
                    ForEach(0..<3, id: \.self) { _ in
                        RoundedRectangle(cornerRadius: 14).fill(theme.surfaceSunk).frame(height: 88)
                    }
                } else if shown.isEmpty {
                    Text("This provider listed the show but returned no episodes for it.")
                        .foregroundStyle(.secondary)
                        .padding(20)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                ForEach(shown, id: \.id) { episode in
                    EpisodeRow(episode: episode, progress: seen[episode.id], compact: !wide) {
                        let at = seen[episode.id].flatMap { !$0.completed && $0.positionSecs > 0 ? Double($0.positionSecs) : nil }
                        playing = .episode(id: episode.id, title: Self.label(episode), startAt: at)
                    } mark: { watched in
                        Task { await setWatched(episode, watched) }
                    }
                }
            }
        }
    }

    private func setWatched(_ episode: Episode, _ watched: Bool) async {
        let was = seen[episode.id]
        seen[episode.id] = EpisodeProgress(episodeId: episode.id, positionSecs: was?.positionSecs ?? 0,
                                           durationSecs: was?.durationSecs ?? 0, completed: watched)
        let profile = model.profileId, id = episode.id
        do {
            try await Task.detached { [core = model.core] in
                try core?.markWatched(profileId: profile, kind: .episode, id: id, watched: watched)
            }.value
        } catch {
            model.error = "Could not change that: \(error.viewerMessage)"
        }
        await loadProgress()
    }

    // MARK: More Like This

    /// Windows' `Similar`: the best-rated titles sharing the first genre.
    @ViewBuilder private var similarGrid: some View {
        if let genre = item.genres.first {
            let found = (similar ?? []).filter { $0.id != item.id }.prefix(9)
            if similar != nil, found.isEmpty {
                note("Nothing else in \(genre) yet.")
            } else {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 12, alignment: .top), count: similarColumns),
                          alignment: .leading, spacing: 12) {
                    if similar == nil {
                        ForEach(0..<4, id: \.self) { _ in
                            RoundedRectangle(cornerRadius: 18).fill(theme.surfaceSunk).aspectRatio(1, contentMode: .fit)
                        }
                    }
                    ForEach(Array(found), id: \.key) { SimilarCard(item: $0) }
                }
            }
        } else {
            note("Nothing to compare this to yet — finding similar titles needs genres, which come from the metadata sweep.")
        }
    }

    private func note(_ words: String) -> some View {
        Text(words)
            .foregroundStyle(.secondary)
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: Details

    private var details: some View {
        let source = alternates.first { $0.id == item.id }?.provider
        var rows: [(String, String)] = [
            ("Title", item.title),
            ("Year", item.year.map(String.init) ?? "—"),
            ("Genres", item.genres.isEmpty ? "—" : item.genres.joined(separator: ", ")),
            ("Certification", item.certification ?? "—"),
            ("Quality", item.quality ?? "—"),
        ]
        if let lang = item.lang { rows.append(("Language", lang)) }
        if let source { rows.append(("Source", source)) }
        return Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 16, verticalSpacing: 8) {
            ForEach(rows, id: \.0) { key, value in
                GridRow {
                    Text(key).foregroundStyle(.tertiary).frame(width: termWidth, alignment: .leading)
                    Text(value).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .font(.ui(.subheadline))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: Loading and playing

    private func loadOnce() async {
        marks = await model.marks(item.listKind, id: item.id)
        alternates = await model.alternates(item.isSeries ? .series : .movies, id: item.id)
        if item.isSeries {
            episodes = await model.episodes(seriesId: item.id, season: nil)
            loadingEpisodes = false
            placeSeason()
        }
        if let genre = item.genres.first {
            let browse = Browse(sort: .rating, category: nil, genre: genre, query: nil, letter: nil, limit: 10, offset: 0)
            similar = item.isSeries
                ? await model.series(browse).map { .series(series: $0) }
                : await model.movies(browse).map { .movie(movie: $0) }
        }
    }

    private func loadProgress() async {
        switch item {
        case .movie(let m):
            progress = await model.progress(.movie, id: m.id)
        case .series(let s):
            resume = await model.seriesResume(seriesId: s.id)
            let profile = model.profileId
            let rows = (try? await Task.detached { [core = model.core] in
                try core?.seriesProgress(profileId: profile, seriesId: s.id) ?? []
            }.value) ?? []
            seen = Dictionary(rows.map { ($0.episodeId, $0) }, uniquingKeysWith: { a, _ in a })
            placeSeason()
        }
    }

    /// Open on the season of the episode the viewer is on, else the first.
    private func placeSeason() {
        guard !placed, !episodes.isEmpty else { return }
        if let resume, let episode = episodes.first(where: { $0.id == resume.episodeId }) {
            season = episode.season
            placed = true
        } else {
            season = seasons.first ?? 1
        }
    }

    private func play() async {
        switch item {
        case .movie(let m):
            playing = .movie(id: m.id, title: m.title, startAt: partway.map { Double($0.positionSecs) })
        case .series(let s):
            await playShow(id: s.id, title: s.title, resume: resume, episodes: episodes)
        }
    }

    /// Another provider's copy: a film from the start, a show where the viewer is in it.
    private func play(copy id: Int64) async {
        switch item {
        case .movie(let m):
            playing = .movie(id: id, title: m.title, startAt: nil)
        case .series(let s):
            await playShow(id: id, title: s.title,
                           resume: await model.seriesResume(seriesId: id),
                           episodes: await model.episodes(seriesId: id, season: nil))
        }
    }

    private func playShow(id: Int64, title: String, resume: SeriesResume?, episodes: [Episode]) async {
        guard let episode = episodes.first(where: { $0.id == resume?.episodeId }) ?? episodes.first else {
            model.error = "No episodes of \(title) have been imported yet"
            return
        }
        let at = resume.flatMap { $0.episodeId == episode.id && $0.positionSecs > 0 ? Double($0.positionSecs) : nil }
        playing = .episode(id: episode.id, title: Self.label(episode), startAt: at)
    }

    /// Windows' `label`: S01E02.
    static func code(_ e: Episode) -> String { String(format: "S%02dE%02d", e.season, e.episode) }

    static func label(_ e: Episode) -> String {
        guard let title = e.title, !title.isEmpty else { return code(e) }
        return "\(code(e)) · \(title)"
    }

    // MARK: Sizes

    private var wide: Bool {
        #if os(tvOS)
        true
        #else
        sizeClass == .regular
        #endif
    }

    private var posterWidth: CGFloat {
        #if os(tvOS)
        440
        #else
        300
        #endif
    }

    private var titleSize: CGFloat {
        #if os(tvOS)
        64
        #else
        wide ? 48 : 34
        #endif
    }

    private var pagePadding: CGFloat {
        #if os(tvOS)
        48
        #else
        wide ? 24 : 16
        #endif
    }

    private var maxWidth: CGFloat {
        #if os(tvOS)
        1640
        #else
        1140
        #endif
    }

    private var similarColumns: Int { wide ? 4 : 2 }

    private var termWidth: CGFloat {
        #if os(tvOS)
        260
        #else
        wide ? 160 : 110
        #endif
    }
}

/// `duration`: 1:02:03, or 2:03 under an hour.
func duration(_ secs: Int64) -> String {
    let s = max(secs, 0)
    let h = s / 3600, m = (s % 3600) / 60, r = s % 60
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, r) : String(format: "%d:%02d", m, r)
}

extension CatalogItem {
    var genres: [String] {
        switch self {
        case .movie(let m): m.genres
        case .series(let s): s.genres
        }
    }

    var rating: Float? {
        switch self {
        case .movie(let m): m.rating
        case .series(let s): s.rating
        }
    }

    var year: Int32? {
        switch self {
        case .movie(let m): m.year
        case .series(let s): s.year
        }
    }

    var certification: String? {
        switch self {
        case .movie(let m): m.certification
        case .series(let s): s.certification
        }
    }

    var quality: String? {
        switch self {
        case .movie(let m): m.quality
        case .series(let s): s.quality
        }
    }

    var cast: [String] {
        switch self {
        case .movie(let m): m.cast
        case .series(let s): s.cast
        }
    }

    var lang: String? {
        switch self {
        case .movie(let m): m.lang
        case .series(let s): s.lang
        }
    }
}

// MARK: - Pieces

/// Windows' `dl-chip`: a small pill on the sunk surface, or on the accent.
private struct Chip: View {
    let text: String
    var accent = false
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Text(text)
            .font(.ui(.caption).weight(.heavy))
            .lineLimit(1)
            .padding(.horizontal, 10)
            .frame(height: 24)
            .foregroundStyle(accent ? theme.accentText : Color.primary)
            .background(accent ? theme.accent : theme.surfaceSunk, in: Capsule())
    }
}

/// A thin bar on a faint track, filled in the accent.
private struct Meter: View {
    let fraction: Double
    let height: CGFloat
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Capsule().fill(Color.primary.opacity(0.15))
            .overlay(alignment: .leading) {
                GeometryReader { geo in
                    Capsule().fill(theme.accent).frame(width: geo.size.width * min(max(fraction, 0), 1))
                }
            }
            .frame(height: height)
    }
}

/// Windows' `IconButton`: a square on the sunk surface, inked when it is on.
struct IconSquare: View {
    let systemImage: String
    let label: String
    let size: CGFloat
    var active = false
    let action: () -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.ui(size: size * 0.4, weight: .semibold))
                #if !os(tvOS)
                .frame(width: size, height: size)
                .foregroundStyle(active ? theme.background : Color.primary)
                .background(active ? Color.primary : theme.surfaceSunk,
                            in: RoundedRectangle(cornerRadius: size >= 44 ? 14 : 8))
                .contentShape(Rectangle())
                #endif
        }
        #if os(tvOS)
        .buttonStyle(.bordered)
        #else
        .buttonStyle(.plain)
        #endif
        .accessibilityLabel(label)
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

#if !os(tvOS)
/// Windows' primary large `Button`: 52 points on the accent.
private struct PrimaryLargeStyle: ButtonStyle {
    @Environment(\.auroraTheme) private var theme

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.ui(.headline).weight(.heavy))
            .labelStyle(.titleAndIcon)
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            .padding(.horizontal, 26)
            .frame(height: 52)
            .foregroundStyle(theme.accentText)
            .background(theme.accent, in: RoundedRectangle(cornerRadius: 14))
            .opacity(configuration.isPressed ? 0.8 : 1)
    }
}
#endif

/// Two columns in proportion, each as tall as it needs, top-aligned (Windows'
/// `1.6fr 1fr`).
private struct RatioColumns: Layout {
    var ratio: CGFloat
    var spacing: CGFloat

    private func widths(_ total: CGFloat) -> (CGFloat, CGFloat) {
        let lead = ((total - spacing) * ratio / (ratio + 1)).rounded()
        return (lead, total - spacing - lead)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 900
        let (a, b) = widths(width)
        let heights = zip(subviews, [a, b]).map { $0.sizeThatFits(.init(width: $1, height: nil)).height }
        return CGSize(width: width, height: heights.max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let (a, b) = widths(bounds.width)
        guard subviews.count == 2 else { return }
        subviews[0].place(at: bounds.origin, proposal: .init(width: a, height: nil))
        subviews[1].place(at: CGPoint(x: bounds.minX + a + spacing, y: bounds.minY), proposal: .init(width: b, height: nil))
    }
}

/// Windows' `Select` for the season.
private struct SeasonMenu: View {
    let seasons: [UInt16]
    @Binding var season: UInt16
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Menu {
            ForEach(seasons, id: \.self) { s in
                Button("Season \(s)") { season = s }
            }
        } label: {
            HStack(spacing: 10) {
                Text("Season \(season)").fontWeight(.bold)
                Image(systemName: "chevron.down").font(.ui(.caption).weight(.bold))
            }
            #if !os(tvOS)
            .foregroundStyle(Color.primary)
            .padding(.horizontal, 14)
            .frame(height: 40)
            .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: 10))
            #endif
        }
        .accessibilityLabel("Season")
        .accessibilityValue("Season \(season)")
    }
}

/// Windows' `PlaybackPrefs`: a show's own switches, as pills, read by the player's skip
/// and Up Next. Per profile.
private struct SeriesPrefsChips: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    let seriesId: Int64
    @State private var prefs: SeriesPrefs?

    private static let switches: [(WritableKeyPath<SeriesPrefs, Bool>, String)] = [
        (\.alwaysSkipIntro, "Always skip intros"),
        (\.alwaysSkipRecap, "Always skip recaps"),
        (\.autoplayNext, "Autoplay next episode"),
    ]

    var body: some View {
        Group {
            if let prefs {
                FlowLayout(spacing: 8) {
                    ForEach(Self.switches, id: \.1) { key, label in
                        chip(label, on: prefs[keyPath: key]) { flip(key, prefs) }
                    }
                }
            } else {
                Color.clear.frame(height: 0)
            }
        }
        .task(id: model.profileId) {
            prefs = try? model.core?.seriesPrefs(profileId: model.profileId, seriesId: seriesId)
        }
    }

    @ViewBuilder
    private func chip(_ label: String, on: Bool, action: @escaping () -> Void) -> some View {
        #if os(tvOS)
        FilterChip(title: label, selected: on, action: action)
        #else
        Button(action: action) {
            Label(label, systemImage: on ? "checkmark" : "plus")
                .font(.ui(.subheadline).weight(.semibold))
                .labelStyle(.titleAndIcon)
                .padding(.horizontal, 14).padding(.vertical, 7)
                .foregroundStyle(on ? theme.accentText : Color.secondary)
                .background(on ? theme.accent : .clear, in: Capsule())
                .overlay { if !on { Capsule().strokeBorder(theme.borderStrong) } }
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(.isToggle)
        .accessibilityValue(on ? "On" : "Off")
        #endif
    }

    private func flip(_ key: WritableKeyPath<SeriesPrefs, Bool>, _ current: SeriesPrefs) {
        var next = current
        next[keyPath: key].toggle()
        prefs = next
        do {
            try model.core?.setSeriesPrefs(profileId: model.profileId, seriesId: seriesId, prefs: next)
        } catch {
            // Put it back: a switch showing what wasn't saved would have the player skip
            // (or not) against what the viewer sees (R-103).
            prefs = current
            model.error = "Could not save that preference: \(error.viewerMessage)"
        }
    }
}

/// One episode: its number (a tick once watched), its still with the bar of how far in,
/// its title, length and overview; and beside the row, the played switch.
private struct EpisodeRow: View {
    let episode: Episode
    let progress: EpisodeProgress?
    let compact: Bool
    let play: () -> Void
    let mark: (Bool) -> Void
    @Environment(\.auroraTheme) private var theme

    private var watched: Bool { progress?.completed ?? false }
    private var partway: Double {
        guard let p = progress, !watched, p.durationSecs > 0 else { return 0 }
        return min(Double(p.positionSecs) / Double(p.durationSecs), 1)
    }

    var body: some View {
        HStack(spacing: 8) {
            Button(action: play) {
                HStack(spacing: 16) {
                    Group {
                        if watched { Image(systemName: "checkmark") } else { Text("\(episode.episode)") }
                    }
                    .font(.ui(.title3).weight(.bold))
                    .foregroundStyle(.tertiary)
                    .frame(width: compact ? 26 : 38)
                    thumb.frame(width: thumbWidth)
                    VStack(alignment: .leading, spacing: 3) {
                        // On a phone the column is narrow: the length goes under the title.
                        let title = Text(episode.title ?? "").font(.ui(.body).weight(.bold))
                            .fixedSize(horizontal: false, vertical: true)
                        let length = Text(runtime(episode.runtimeMins)).font(.ui(.subheadline)).foregroundStyle(.tertiary)
                        if compact {
                            title
                            length
                        } else {
                            HStack(alignment: .firstTextBaseline, spacing: 12) {
                                title
                                Spacer(minLength: 0)
                                length
                            }
                        }
                        if let overview = episode.overview, !overview.isEmpty {
                            Text(overview).font(.ui(.subheadline)).foregroundStyle(.secondary).lineLimit(2)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(12)
                .opacity(watched ? 0.55 : 1)
                .contentShape(Rectangle())
            }
            .buttonStyle(RowStyle())
            .accessibilityLabel("\(TitleDetail.code(episode)) \(episode.title ?? "")")
            IconSquare(systemImage: "checkmark",
                       label: watched ? "Mark \(TitleDetail.code(episode)) as unplayed" : "Mark \(TitleDetail.code(episode)) as played",
                       size: 34, active: watched) { mark(!watched) }
        }
    }

    /// A box of its own size whether or not there is a still, so the play mark and the
    /// resume bar always show (Windows' R-106).
    private var thumb: some View {
        Rectangle()
            .fill(Color.primary.opacity(0.06))
            .aspectRatio(16 / 9, contentMode: .fit)
            .overlay {
                if let still = episode.still, let url = URL(string: still) {
                    AsyncImage(url: url) { $0.resizable().scaledToFill() } placeholder: { Color.clear }
                }
            }
            .overlay {
                ZStack {
                    Color.black.opacity(0.3)
                    Image(systemName: "play.fill").font(.ui(.title3)).foregroundStyle(.white)
                }
            }
            .overlay(alignment: .bottom) {
                if partway > 0 {
                    ZStack(alignment: .leading) {
                        Rectangle().fill(.white.opacity(0.25))
                        GeometryReader { geo in
                            Rectangle().fill(theme.accent).frame(width: geo.size.width * partway)
                        }
                    }
                    .frame(height: 3)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private var thumbWidth: CGFloat {
        #if os(tvOS)
        240
        #else
        compact ? 112 : 148
        #endif
    }
}

/// An episode row: the surface under it while pressed, or white while focused on a TV.
private struct RowStyle: ButtonStyle {
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #endif

    func makeBody(configuration: Configuration) -> some View {
        #if os(tvOS)
        configuration.label
            .foregroundStyle(focused ? Color.black : Color.primary)
            .background(focused ? Color.white : .clear, in: RoundedRectangle(cornerRadius: 14))
            .scaleEffect(focused ? 1.02 : 1)
            .animation(.easeOut(duration: 0.15), value: focused)
        #else
        configuration.label
            .foregroundStyle(Color.primary)
            .background(configuration.isPressed ? theme.surfaceSunk : .clear, in: RoundedRectangle(cornerRadius: 14))
        #endif
    }
}

/// Windows' similar card: square art, the title, and the year with the runtime or the
/// seasons. Not a link, as on Windows; on a TV it takes focus, so the remote can reach
/// the rows below.
private struct SimilarCard: View {
    let item: CatalogItem
    #if os(tvOS)
    @FocusState private var focused: Bool
    #endif

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Color.clear
                .aspectRatio(1, contentMode: .fit)
                .background(ChannelTile.hue(item.title))
                .overlay {
                    if let art = item.backdrop, let url = URL(string: art) {
                        AsyncImage(url: url) { $0.resizable().scaledToFill() } placeholder: { Color.clear }
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 18))
                #if os(tvOS)
                .scaleEffect(focused ? 1.05 : 1)
                .shadow(radius: focused ? 12 : 0)
                .animation(.easeOut(duration: 0.15), value: focused)
                #endif
            Text(item.title)
                .font(.ui(.subheadline).weight(.heavy))
                .padding(.top, 8).padding(.bottom, 2)
            Text(subtitle)
                .font(.ui(.caption).weight(.bold))
                .foregroundStyle(.tertiary)
        }
        #if os(tvOS)
        .focusable()
        .focused($focused)
        #endif
        .accessibilityElement(children: .combine)
    }

    private var subtitle: String {
        var parts: [String] = []
        if let year = item.year { parts.append(String(year)) }
        switch item {
        case .movie(let m): if !runtime(m.runtimeMins).isEmpty { parts.append(runtime(m.runtimeMins)) }
        case .series(let s): parts.append(s.seasons.count == 1 ? "1 season" : "\(s.seasons.count) seasons")
        }
        return parts.joined(separator: " · ")
    }
}
