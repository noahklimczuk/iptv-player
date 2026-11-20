import SwiftUI

/// One search across channels, the guide, films, series and people: Windows' search
/// palette (`CommandPalette`), with its groups in its order and words, four hits to a
/// group until "Show N more", and what a hit opens.
struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var results: SearchResults?
    /// Groups opened past their first four, by key; closed again on new text.
    @State private var expanded: Set<String> = []
    @State private var playing: PlayerItem?
    @State private var movie: Movie?
    /// Shown as a sheet (an iPhone), so it needs its own way out.
    var closable = false
    @Environment(\.dismiss) private var dismiss

    /// Windows' `PER_GROUP`.
    private static let perGroup = 4

    var body: some View {
        NavigationStack {
            List {
                if let results {
                    ForEach(Self.groups(results), id: \.key) { group in
                        section(group)
                    }
                    let total = Self.groups(results).reduce(0) { $0 + $1.hits.count }
                    if total > 0 {
                        Section {} footer: {
                            Text("\(total) results")
                                .font(.ui(.caption).weight(.semibold))
                                .foregroundStyle(.secondary)
                                .frame(maxWidth: .infinity, alignment: .trailing)
                        }
                    }
                }
            }
            .overlay {
                if text.trimmingCharacters(in: .whitespaces).isEmpty {
                    note("Type to search across live channels, the next 7 days of guide data, your movies, series, and cast.")
                } else if let results, Self.groups(results).allSatisfy(\.hits.isEmpty) {
                    note("Nothing matches “\(text)”.")
                }
            }
            .auroraPageTitle("Search")
            .toolbar {
                if closable {
                    ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                }
            }
            .searchable(text: $text, prompt: "Search channels, guide, movies, series, people…")
            .task(id: text) {
                expanded = []
                // Wait for typing to pause before asking.
                try? await Task.sleep(for: .milliseconds(90))
                guard !Task.isCancelled else { return }
                results = text.trimmingCharacters(in: .whitespaces).isEmpty ? nil : await model.search(text)
            }
            .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
            .sheet(item: $movie) { MovieDetail(movie: $0).auroraAppearance() }
            // A title handed over by "Search this title", now or while already open.
            .task(id: model.searchQuery) {
                guard let query = model.searchQuery else { return }
                model.searchQuery = nil
                text = query
            }
        }
    }

    // MARK: Groups

    /// What a hit opens. A programme hit's subtitle is its channel's name.
    private enum Opens { case channel, programme, movie, series, nothing }

    private struct Group {
        let key: String
        let label: String
        let icon: String
        let hits: [SearchHit]
        let opens: Opens
    }

    /// Windows' `GROUPS`, in its order. A programme, on now or later, opens its channel
    /// (its hit names the channel); a person opens nothing, as on Windows.
    private static func groups(_ r: SearchResults) -> [Group] {
        [
            Group(key: "channels", label: "Live Channels", icon: "tv", hits: r.channels, opens: .channel),
            Group(key: "movies", label: "Movies", icon: "film", hits: r.movies, opens: .movie),
            Group(key: "series", label: "Series", icon: "rectangle.stack", hits: r.series, opens: .series),
            Group(key: "onNow", label: "On Now", icon: "clock", hits: r.onNow, opens: .programme),
            Group(key: "upcoming", label: "Upcoming", icon: "bell", hits: r.upcoming, opens: .programme),
            Group(key: "people", label: "People", icon: "person", hits: r.people, opens: .nothing),
        ]
    }

    @ViewBuilder private func section(_ group: Group) -> some View {
        if !group.hits.isEmpty {
            let shown = expanded.contains(group.key) ? group.hits : Array(group.hits.prefix(Self.perGroup))
            let folded = group.hits.count - shown.count
            Section {
                ForEach(Array(shown.enumerated()), id: \.offset) { _, hit in
                    row(hit, opens: group.opens)
                }
                if folded > 0 {
                    Button("Show \(folded) more \(group.label.lowercased())") { expanded.insert(group.key) }
                        .font(.ui(.subheadline).weight(.bold))
                        #if !os(tvOS)
                        .foregroundStyle(.tint)
                        #endif
                }
            } header: {
                HStack(spacing: 6) {
                    Image(systemName: group.icon)
                    Text(group.label)
                    Text("\(group.hits.count)").opacity(0.6)
                }
                .font(.ui(.subheadline).weight(.heavy))
                .accessibilityElement(children: .combine)
            }
        }
    }

    @ViewBuilder private func row(_ hit: SearchHit, opens: Opens) -> some View {
        let label = HStack(spacing: 12) {
            Text(hit.title)
                .fontWeight(.bold)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let subtitle = hit.subtitle {
                Text(subtitle)
                    .font(.ui(.caption).weight(.semibold))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        switch opens {
        case .channel, .programme:
            let name = opens == .programme ? hit.subtitle ?? hit.title : hit.title
            Button { playing = .channel(id: hit.id, name: name) } label: { label }
                #if !os(tvOS)
                .foregroundStyle(Color.primary)
                #endif
        case .movie:
            Button { Task { movie = await model.movieDetail(id: hit.id) } } label: { label }
                #if !os(tvOS)
                .foregroundStyle(Color.primary)
                #endif
        case .series:
            NavigationLink { SeriesDetail(seriesId: hit.id) } label: { label }
        case .nothing:
            label
        }
    }

    /// Windows' faint centred line, for nothing typed and for nothing found.
    private func note(_ words: String) -> some View {
        Text(words)
            .font(.ui(.subheadline))
            .foregroundStyle(.tertiary)
            .multilineTextAlignment(.center)
            .padding(.horizontal, 32)
            .frame(maxWidth: 560)
    }
}
