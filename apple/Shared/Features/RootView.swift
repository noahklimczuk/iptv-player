import SwiftUI

/// The app's top level: Live TV, Guide, Films, Series, Settings, and Search.
///
/// A `TabView` of `Tab`s: on tvOS the top bar the Siri Remote expects, on iPhone and
/// iPad the floating tab bar. Search has the search role, so iOS 26 gives it its own
/// button beside the bar. The app targets iOS 26 and tvOS 26 only.
struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var tab: AppTab = .live
    #if DEBUG
    @State private var demo: PlayerItem?
    #endif

    enum AppTab: String { case live, guide, films, series, settings, search }

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
        TabView(selection: $tab) {
            Tab("Live TV", systemImage: "tv", value: .live) { LiveView() }
            Tab("Guide", systemImage: "calendar", value: .guide) { GuideView() }
            Tab("Films", systemImage: "film", value: .films) { MoviesView() }
            Tab("Series", systemImage: "rectangle.stack", value: .series) { SeriesView() }
            Tab("Settings", systemImage: "gear", value: .settings) { SettingsView() }
            Tab(value: AppTab.search, role: .search) { SearchView() }
        }
        #if DEBUG
        // `-AuroraTab guide` at launch opens that tab (see DemoLaunch).
        .onAppear {
            if let name = UserDefaults.standard.string(forKey: "AuroraTab"), let start = AppTab(rawValue: name) {
                tab = start
            }
        }
        #endif
        .task { await model.reload() }
        // Here rather than on one tab, so a failure is seen whichever tab is open: an
        // import started from Live TV can fail while the Guide is showing.
        .alert("Something went wrong", isPresented: errorShown) {
            Button("OK") { model.error = nil }
        } message: {
            Text(model.error ?? "")
        }
    }

    private var errorShown: Binding<Bool> {
        Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })
    }
}
