import SwiftUI

/// The app's top level: Live TV, Films, Series, Search, Settings.
///
/// A `TabView` of `Tab`s: on tvOS the top bar the Siri Remote expects, on iPhone and
/// iPad the floating tab bar. Search has the search role, so iOS 26 gives it its own
/// button beside the bar. The app targets iOS 26 and tvOS 26 only.
struct RootView: View {
    @Environment(AppModel.self) private var model
    #if DEBUG
    @State private var demo: PlayerItem?
    #endif

    var body: some View {
        Group {
            if !model.profilesLoaded {
                ProgressView()
            } else if model.picking && !model.profiles.isEmpty {
                ProfilePicker()
            } else {
                tabs
                    // A new profile starts every screen afresh: its favourites, its My
                    // List, its progress.
                    .id(model.profileId)
            }
        }
        .task { await model.loadProfiles() }
        #if DEBUG
        .task { demo = await DemoLaunch.prepare(model) }
        .fullScreenCover(item: $demo) { PlayerScreen(item: $0) }
        #endif
    }

    private var tabs: some View {
        TabView {
            Tab("Live TV", systemImage: "tv") { LiveView() }
            Tab("Films", systemImage: "film") { MoviesView() }
            Tab("Series", systemImage: "rectangle.stack") { SeriesView() }
            Tab("Settings", systemImage: "gear") { SettingsView() }
            Tab(role: .search) { SearchView() }
        }
        .task { await model.reload() }
    }
}
