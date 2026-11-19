import SwiftUI

/// One search across channels, the guide, films and series.
struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var results: SearchResults?
    @State private var playing: PlayerItem?
    @State private var movie: Movie?
    /// Shown as a sheet (an iPhone), so it needs its own way out.
    var closable = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                if let r = results {
                    section("Channels", r.channels) { hit in
                        playing = .channel(id: hit.id, name: hit.title)
                    }
                    section("On now", r.onNow) { hit in
                        // A programme hit names its channel: play that.
                        playing = .channel(id: hit.id, name: hit.subtitle ?? hit.title)
                    }
                    section("Later", r.upcoming, action: nil)
                    section("Movies", r.movies) { hit in
                        playing = .movie(id: hit.id, title: hit.title, startAt: nil)
                    }
                    if !r.series.isEmpty {
                        Section("Series") {
                            ForEach(r.series, id: \.id) { hit in
                                NavigationLink(hit.title) { SeriesDetail(seriesId: hit.id) }
                            }
                        }
                    }
                }
            }
            .overlay {
                if let r = results, r.channels.isEmpty, r.onNow.isEmpty, r.upcoming.isEmpty,
                   r.movies.isEmpty, r.series.isEmpty {
                    ContentUnavailableView.search(text: text)
                }
            }
            .auroraPageTitle("Search")
            .toolbar {
                if closable {
                    ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                }
            }
            .searchable(text: $text, prompt: "Channels, programmes, films, series")
            .task(id: text) {
                // Wait for typing to pause before asking.
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled else { return }
                results = text.trimmingCharacters(in: .whitespaces).isEmpty ? nil : await model.search(text)
            }
            .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
            // A title handed over by "Search this title", now or while already open.
            .task(id: model.searchQuery) {
                guard let query = model.searchQuery else { return }
                model.searchQuery = nil
                text = query
            }
        }
    }

    @ViewBuilder
    private func section(_ title: String, _ hits: [SearchHit], action: ((SearchHit) -> Void)?) -> some View {
        if !hits.isEmpty {
            Section(title) {
                ForEach(hits, id: \.self) { hit in
                    let row = VStack(alignment: .leading) {
                        Text(hit.title)
                        if let subtitle = hit.subtitle {
                            Text(subtitle).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    if let action {
                        Button { action(hit) } label: { row }
                    } else {
                        row
                    }
                }
            }
        }
    }
}
