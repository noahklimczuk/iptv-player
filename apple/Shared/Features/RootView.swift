import SwiftUI

/// The app's top level: Home, Live TV, Guide, Films, Series, Recordings, the assistant,
/// Settings, and Search.
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
    @State private var showingRecordings = false
    /// The setup wizard was finished, or its import left running. Not stored: a library
    /// with no provider asks again next launch, as on Windows.
    @State private var setupFinished = false
    /// "Skip for now": this run only, which is what the button says.
    @State private var setupSkipped = false
    @Environment(\.scenePhase) private var scenePhase
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    #if DEBUG
    @State private var demo: PlayerItem?
    #endif

    enum AppTab: String { case home, live, guide, films, series, recordings, assistant, settings, search }

    var body: some View {
        Group {
            if !model.profilesLoaded || !model.providersLoaded {
                ProgressView()
            } else if needsSetup {
                // Before "Who's watching?": there is no point choosing a profile for an
                // empty library.
                SetupWizard(onFinished: { setupFinished = true }, onSkip: { setupSkipped = true })
            } else if model.picking && !model.profiles.isEmpty {
                ProfilePicker()
            } else {
                tabs
                    // A new profile starts every screen afresh: its favourites, its My
                    // List, its progress.
                    .id(model.profileId)
            }
        }
        // The parental PIN dialog, wherever the core asks for it (`ParentalPinHost`).
        .parentalPinHost()
        .task { await model.loadProfiles() }
        .task { await model.reload() }
        .task { await model.loadParental() }
        // The recordings scheduler keeps its own time while Aurora runs, as Windows'
        // does while its window is open.
        .task { model.startRecorder() }
        .task { await model.runPrivacySweep() }
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
        // Windows' `ConnectionBudget` heads the page while recordings use every stream
        // there is. Above the tabs, not inset into them: a navigation bar's large title
        // ignores an inset from outside its stack and would sit under the strip.
        VStack(spacing: 0) {
            ConnectionBudget()
            tabView
        }
        // An iPhone's bar holds five, the search tab included; past that the rest go
        // under "More", whose own navigation bar stacks on top of the tab's and doubles
        // every back button. So there, as in Apple's TV app, Search and Settings open
        // from Home's toolbar.
        .environment(\.openSettings, compact ? { showingSettings = true } : nil)
        .environment(\.openSearch, compact ? { showingSearch = true } : nil)
        .environment(\.openRecordings, compact ? { showingRecordings = true } : nil)
        // Its own tab where there is room (Windows' nav has one); a sheet from Home's
        // toolbar on an iPhone. `question` is asked as soon as it opens.
        .environment(\.openAssistant, { question in
            model.assistantQuestion = question
            if compact { showingAssistant = true } else { tab = .assistant }
        })
        .sheet(isPresented: $showingAssistant) { AssistantView(closable: true) }
        .sheet(isPresented: $showingSettings) { SettingsView(closable: true) }
        .sheet(isPresented: $showingSearch) { SearchView(closable: true) }
        .sheet(isPresented: $showingRecordings) { RecordingsView(closable: true) }
        #if DEBUG
        // `-AuroraTab guide` at launch opens that tab (see DemoLaunch).
        .onAppear {
            if let name = UserDefaults.standard.string(forKey: "AuroraTab"), let start = AppTab(rawValue: name) {
                if compact && start == .settings { showingSettings = true }
                else if compact && start == .assistant { showingAssistant = true }
                else if compact && start == .search { showingSearch = true }
                else if compact && start == .recordings { showingRecordings = true }
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

    private var tabView: some View {
        TabView(selection: $tab) {
            Tab("Home", systemImage: "house", value: .home) { HomeView() }
            Tab("Live TV", systemImage: "tv", value: .live) { LiveView() }
            Tab("Guide", systemImage: "calendar", value: .guide) { GuideView() }
            Tab("Films", systemImage: "film", value: .films) { MoviesView() }
            Tab("Series", systemImage: "rectangle.stack", value: .series) { SeriesView() }
            if !compact {
                Tab("Recordings", systemImage: "record.circle", value: .recordings) { RecordingsView() }
                Tab("Assistant", systemImage: "sparkles", value: .assistant) { AssistantView() }
                Tab("Settings", systemImage: "gear", value: .settings) { SettingsView() }
                Tab(value: AppTab.search, role: .search) { SearchView() }
            }
        }
    }

    /// No provider yet, and the wizard neither finished nor skipped this run.
    private var needsSetup: Bool {
        guard !setupFinished && !setupSkipped else { return false }
        #if DEBUG
        // `-AuroraSetup YES` shows it over a library that has providers, for demos and
        // tests (Windows' `?setup`).
        if UserDefaults.standard.bool(forKey: "AuroraSetup") { return true }
        #endif
        return model.providers.isEmpty
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
    /// Opens Recordings where it isn't a tab (an iPhone); nil where it is.
    @Entry var openRecordings: (() -> Void)? = nil
    /// Opens the assistant, asking `question` first when there is one.
    @Entry var openAssistant: (String?) -> Void = { _ in }
}

/// A strip that appears while recordings are using every stream the subscription allows
/// (Windows' `ConnectionBudget`): watching something opens a connection too, and on a
/// one-stream account a recording in flight means the next thing pressed cannot play.
/// Visible only while it is true.
private struct ConnectionBudget: View {
    @Environment(AppModel.self) private var model
    @State private var allowed: UInt32 = 0
    @State private var inFlight: [Recording] = []

    var body: some View {
        Group {
            if allowed > 0 && inFlight.count >= Int(allowed) {
                let names = inFlight.map(\.title)
                let what = names.count == 1 ? names[0] : "\(names.count) recordings"
                HStack(spacing: 8) {
                    Image(systemName: "record.circle.fill").font(.caption)
                    (Text(what).bold().foregroundStyle(.primary)
                     + Text(" ")
                     + Text(allowed == 1
                            ? "is using the only stream this subscription allows"
                            : "are using all \(allowed) streams this subscription allows")
                     + Text(". Starting something now may stop \(names.count == 1 ? "it" : "one of them")."))
                }
                .font(.callout)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12).padding(.vertical, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
                .padding(.horizontal)
                .padding(.vertical, 4)
                .accessibilityElement(children: .combine)
            }
        }
        // On the scheduler's tick, as Windows' strip follows `dvr.tick`.
        .task(id: model.recordingsVersion) {
            allowed = await model.dvrStorage()?.maxConcurrent ?? 0
            inFlight = await model.recordingsInFlight()
        }
    }
}
