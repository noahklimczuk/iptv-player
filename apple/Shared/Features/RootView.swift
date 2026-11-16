import SwiftUI

/// The app's top level: Live TV, Films, Series, Search, Settings.
///
/// A `TabView`: on tvOS the top bar the Siri Remote expects, on iPhone and iPad the tab
/// bar.
struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        TabView {
            LiveView().tabItem { Label("Live TV", systemImage: "tv") }
            MoviesView().tabItem { Label("Films", systemImage: "film") }
            SeriesView().tabItem { Label("Series", systemImage: "rectangle.stack") }
            SearchView().tabItem { Label("Search", systemImage: "magnifyingglass") }
            SettingsView().tabItem { Label("Settings", systemImage: "gear") }
        }
        .task { await model.reload() }
    }
}
