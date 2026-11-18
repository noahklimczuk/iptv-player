import SwiftUI

/// The app's top level: Live TV, Guide, Films, Series, Settings, and Search.
///
/// A `TabView` of `Tab`s: on tvOS the top bar the Siri Remote expects, on iPhone and
/// iPad the floating tab bar. Search has the search role, so iOS 26 gives it its own
/// button beside the bar. The app targets iOS 26 and tvOS 26 only.
struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var tab: AppTab = .home
    @State private var showingSettings = false
    @State private var showingSearch = false
    @State private var showingAssistant = false
    @Environment(\.scenePhase) private var scenePhase
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    #if DEBUG
    @State private var demo: PlayerItem?
    #endif

    enum AppTab: String { case home, live, guide, films, series, assistant, settings, search }

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
        // Sync while the app is in front; a new phase cancels the loop and, if active
        // again, starts it afresh.
        .task(id: scenePhase) {
            if scenePhase == .active { await model.runSyncLoop() }
        }
        #if DEBUG
        .task { demo = await DemoLaunch.prepare(model) }
        .fullScreenCover(item: $demo) { PlayerScreen(item: $0) }
        #endif
    }

    private var tabs: some View {
        TabView(selection: $tab) {
            Tab("Home", systemImage: "house", value: .home) { HomeView() }
            Tab("Live TV", systemImage: "tv", value: .live) { LiveView() }
            Tab("Guide", systemImage: "calendar", value: .guide) { GuideView() }
            Tab("Films", systemImage: "film", value: .films) { MoviesView() }
            Tab("Series", systemImage: "rectangle.stack", value: .series) { SeriesView() }
            if !compact {
                Tab("Assistant", systemImage: "sparkles", value: .assistant) { AssistantView() }
                Tab("Settings", systemImage: "gear", value: .settings) { SettingsView() }
                Tab(value: AppTab.search, role: .search) { SearchView() }
            }
        }
        // An iPhone's bar holds five, the search tab included; past that the rest go
        // under "More", whose own navigation bar stacks on top of the tab's and doubles
        // every back button. So there, as in Apple's TV app, Search and Settings open
        // from Home's toolbar.
        .environment(\.openSettings, compact ? { showingSettings = true } : nil)
        .environment(\.openSearch, compact ? { showingSearch = true } : nil)
        // Its own tab where there is room (Windows' nav has one); a sheet from Home's
        // toolbar on an iPhone. `question` is asked as soon as it opens.
        .environment(\.openAssistant, { question in
            model.assistantQuestion = question
            if compact { showingAssistant = true } else { tab = .assistant }
        })
        .sheet(isPresented: $showingAssistant) { AssistantView(closable: true) }
        .sheet(isPresented: $showingSettings) { SettingsView(closable: true) }
        .sheet(isPresented: $showingSearch) { SearchView(closable: true) }
        #if DEBUG
        // `-AuroraTab guide` at launch opens that tab (see DemoLaunch).
        .onAppear {
            if let name = UserDefaults.standard.string(forKey: "AuroraTab"), let start = AppTab(rawValue: name) {
                if compact && start == .settings { showingSettings = true }
                else if compact && start == .assistant { showingAssistant = true }
                else if compact && start == .search { showingSearch = true }
                else { tab = start }
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

    /// An iPhone, or an iPad in a narrow window.
    private var compact: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }

    private var errorShown: Binding<Bool> {
        Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })
    }
}

extension EnvironmentValues {
    /// Opens Settings where it isn't a tab (an iPhone); nil where it is.
    @Entry var openSettings: (() -> Void)? = nil
    /// Opens Search where it isn't a tab (an iPhone); nil where it is.
    @Entry var openSearch: (() -> Void)? = nil
    /// Opens the assistant, asking `question` first when there is one.
    @Entry var openAssistant: (String?) -> Void = { _ in }
}
