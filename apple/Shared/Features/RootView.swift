import SwiftUI

/// The app's top level: Home, Live TV, Guide, Movies, Series, Recordings, Multi-view, the
/// assistant, the playlist, Settings, and Search.
///
/// Windows' tab bar (`AuroraTabBar`) at the foot of every screen, over the page it has
/// chosen, which is built afresh each time as Windows' routes are. Search is not a tab
/// on Windows either: it opens from Home. The app targets iOS 26 and tvOS 26 only.
struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var tab: AppTab = .home
    @State private var showingSearch = false
    /// The top bar's height, with the connection strip when it shows.
    @State private var headerHeight: CGFloat = AuroraTopBar.height
    /// The phone's More panel is open over the page.
    @State private var moreOpen = false
    #if os(tvOS)
    /// The tab the focus is on in the bar, or nil while it is on the page.
    @FocusState private var barFocus: AppTab?
    #endif
    #if DEBUG
    @State private var debugPairing = false
    #endif
    /// The setup wizard was finished, or its import left running. Not stored: a library
    /// with no provider asks again next launch, as on Windows.
    @State private var setupFinished = false
    /// "Skip for now": this run only, which is what the button says.
    @State private var setupSkipped = false
    /// The wizard is up. It stays until it is finished or skipped: the provider it saves
    /// would otherwise end it there and then, mid-import and before its Done step.
    @State private var setupShown = false
    /// A Settings section to open at (Windows' `#/settings?section=`), as the setup
    /// wizard's offer of an account asks for.
    @State private var settingsSection: String?
    @Environment(\.scenePhase) private var scenePhase
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    #if DEBUG
    @State private var demo: PlayerItem?
    #endif

    enum AppTab: String { case home, live, guide, films, series, recordings, multiview, assistant, playlist, settings, search }

    var body: some View {
        Group {
            if !model.profilesLoaded || !model.providersLoaded {
                ProgressView()
            } else if needsSetup {
                // Before "Who's watching?": there is no point choosing a profile for an
                // empty library.
                SetupWizard(onFinished: { setupFinished = true }, onSkip: { setupSkipped = true },
                            onSync: {
                                setupFinished = true
                                tab = .settings
                                settingsSection = SyncSection.title
                            })
                    .onAppear { setupShown = true }
            } else if model.picking && !model.profiles.isEmpty {
                ProfilePicker()
            } else {
                tabs
                    // A new profile starts every screen afresh: its favourites, its My
                    // List, its progress.
                    .id(model.profileId)
            }
        }
        // The whole screen, so the theme's background below fills it behind a screen
        // that hugs its content ("Who's watching?") rather than leaving the window's own
        // colour in bands above and below.
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // The parental PIN dialog, wherever the core asks for it (`ParentalPinHost`).
        .parentalPinHost()
        // Settings → Appearance: the theme, the clock and motion, for every screen.
        .auroraAppearance(root: true)
        .task { await model.loadProfiles() }
        .task { await model.reload() }
        #if DEBUG
        // `-AuroraPairing YES`: either end's pairing screen, even in a build with no sync
        // service, to try the transport against the other end (or a script) on the network.
        #if os(tvOS)
        .fullScreenCover(isPresented: $debugPairing) {
            PairWithPhoneView(deviceName: "Apple TV") {}.auroraAppearance()
        }
        #else
        .sheet(isPresented: $debugPairing) {
            SetUpAppleTVView(email: "household@example.com").auroraAppearance()
        }
        #endif
        .onAppear { debugPairing = UserDefaults.standard.bool(forKey: "AuroraPairing") }
        #endif
        // The Top Shelf's titles: Home, with the title's page open over it.
        .onOpenURL { url in
            guard let link = TopShelfSnapshot.title(from: url) else { return }
            showingSearch = false
            tab = .home
            model.titleToOpen = TitleLink(kind: link.kind, id: link.id)
        }
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
        // Windows' top bar heads every tab, with its `ConnectionBudget` under it while
        // recordings use every stream there is. It floats over the page, which scrolls
        // on beneath its fade as Windows' does. A navigation stack ignores a SwiftUI
        // inset from outside it, so `StackInset` gives each stack the bar's height as
        // UIKit safe area instead: its bar and first row start below ours, and its
        // content still scrolls up under it. (The tab bar's space at the foot, likewise,
        // in `tabView`.)
        ZStack(alignment: .top) {
            tabView.auroraRebuildOnSwitch()
            VStack(spacing: 0) {
                AuroraTopBar(title: pageTitle)
                ConnectionBudget()
            }
            // Only a real change: a fraction of a point either way is layout noise, and
            // each change lays every page out again (see `StackInset.Probe.apply`).
            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height in
                if abs(height - headerHeight) >= 0.5 { headerHeight = height }
            }
        }
        // Search opens over the page from Home's toolbar, as Windows' opens from its top
        // bar; every other destination is a tab, on a phone as well.
        .environment(\.openSearch, { showingSearch = true })
        // "Search this title": Search, with the title already typed.
        .environment(\.searchFor, { query in
            model.searchQuery = query
            showingSearch = true
        })
        // `question` is asked as soon as the assistant opens.
        .environment(\.openAssistant, { question in
            model.assistantQuestion = question
            tab = .assistant
        })
        // The player's TV Guide button: whatever sheet the player was opened from goes
        // with it.
        .onChange(of: model.guideRequests) {
            showingSearch = false
            tab = .guide
        }
        // Its own presentation, which keeps the system's appearance unless given the
        // theme itself.
        .sheet(isPresented: $showingSearch) { SearchView(closable: true).auroraAppearance() }
        #if DEBUG
        // `-AuroraTab guide` at launch opens that tab (see DemoLaunch).
        .onAppear {
            if let name = UserDefaults.standard.string(forKey: "AuroraTab"), let start = AppTab(rawValue: name) {
                if start == .search { showingSearch = true } else { tab = start }
            }
            // `-AuroraSettingsSection "Account and sync"` opens Settings there.
            settingsSection = UserDefaults.standard.string(forKey: "AuroraSettingsSection")
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
        GeometryReader { geo in
            let edge = geo.safeAreaInsets.bottom
            page
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                // The page scrolls on beneath the bar, clear of it at the end
                // (`--navpill-space`).
                .safeAreaPadding(.bottom, barSpace(edge))
                // The same for a navigation stack, which ignores the line above: without
                // it what a page pins to its foot, as Ask Aurora's box, sits under the bar.
                .background(StackInset(top: headerHeight, bottom: barSpace(edge)).id(tab))
                #if os(tvOS)
                // The Menu button, as under the system's tab bar: on a page it goes to
                // the bar, on the current tab; on the bar it leaves the app (no handler,
                // so the system's own). A stack's pushed page still goes back first.
                .onExitCommand(perform: barFocus == nil ? { barFocus = tab } : nil)
                #endif
                #if os(iOS)
                // More's panel is open: the page dims a little, and a tap on it closes the
                // panel rather than reaching the page.
                .overlay {
                    if moreOpen {
                        Color.black.opacity(0.22)
                            .ignoresSafeArea()
                            .contentShape(Rectangle())
                            .onTapGesture { moreOpen = false }
                            .transition(.opacity)
                            .accessibilityLabel("Close More")
                            .accessibilityAddTraits(.isButton)
                    }
                }
                .animation(.easeOut(duration: 0.2), value: moreOpen)
                .onChange(of: tab) { moreOpen = false }
                #endif
                .overlay(alignment: .bottom) {
                    tabBar.offset(y: barDrop(edge))
                }
        }
        #if os(iOS)
        // The keyboard rises over the bar rather than lifting it.
        .ignoresSafeArea(.keyboard, edges: .bottom)
        #endif
    }

    private var tabBar: some View {
        #if os(tvOS)
        AuroraTabBar(items: navItems, selection: $tab, focus: $barFocus)
        #else
        // A phone fits the bar to its screen and puts the rest under More.
        AuroraTabBar(items: navItems, selection: $tab, more: compact ? $moreOpen : nil)
        #endif
    }

    /// Windows' bar floats 20 points above the foot of the window, so on a phone it sits
    /// down in the home indicator's margin, as the system's own bar does. On a TV it stays
    /// inside the safe area, which keeps it clear of the screen's overscan.
    private func barDrop(_ edge: CGFloat) -> CGFloat {
        #if os(tvOS)
        0
        #else
        edge - 20
        #endif
    }

    /// What the page keeps clear above the safe area: the bar, its gap from the edge and
    /// 12 points more (Windows' `--navpill-space`, 104, or 132 in TV mode).
    private func barSpace(_ edge: CGFloat) -> CGFloat {
        #if os(tvOS)
        AuroraTabBar<AppTab>.height + 12
        #else
        max(0, 104 - edge)
        #endif
    }

    /// Windows' `NAV`, in its order. Not Multi-view on a phone, as the port's plan has it
    /// (docs/APPLE_PORT_AND_SYNC.md §2): a phone has neither the screen for four channels
    /// nor the heat budget for four decoders.
    private var navItems: [AuroraTabBar<AppTab>.Item] {
        var items: [AuroraTabBar<AppTab>.Item] = [
            .init(value: .home, label: "Home", icon: .home),
            .init(value: .live, label: "Live TV", icon: .tv),
            .init(value: .guide, label: "Guide", icon: .grid),
            .init(value: .films, label: "Movies", icon: .film),
            .init(value: .series, label: "Series", icon: .stack),
            .init(value: .recordings, label: "Recordings", icon: .record),
            .init(value: .multiview, label: "Multi-view", icon: .pip),
            .init(value: .assistant, label: "Assistant", icon: .sparkle),
            .init(value: .playlist, label: "Playlist", icon: .layers),
            .init(value: .settings, label: "Settings", icon: .settings),
        ]
        if compact { items.removeAll { $0.value == .multiview } }
        return items
    }

    /// The top bar's name for the page (Windows' `titleFor`): its tab's label.
    private var pageTitle: String {
        navItems.first { $0.value == tab }?.label ?? (tab == .search ? "Search" : "Aurora TV")
    }

    /// The page the bar has chosen.
    @ViewBuilder private var page: some View {
        switch tab {
        case .home: HomeView()
        case .live: LiveView()
        case .guide: GuideView()
        case .films: MoviesView()
        case .series: SeriesView()
        case .recordings: RecordingsView()
        case .multiview: if compact { HomeView() } else { MultiViewPage() }
        case .assistant: AssistantView()
        case .playlist: NavigationStack { PlaylistEditorView() }
        case .settings: SettingsView(section: $settingsSection)
        case .search: SearchView()
        }
    }

    /// No provider yet, and the wizard neither finished nor skipped this run.
    private var needsSetup: Bool {
        guard !setupFinished && !setupSkipped else { return false }
        if setupShown { return true }
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
    /// Opens Search with `query` typed in.
    @Entry var searchFor: (String) -> Void = { _ in }
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
                    Image(systemName: "record.circle.fill").font(.ui(.caption))
                    (Text(what).bold().foregroundStyle(.primary)
                     + Text(" ")
                     + Text(allowed == 1
                            ? "is using the only stream this subscription allows"
                            : "are using all \(allowed) streams this subscription allows")
                     + Text(". Starting something now may stop \(names.count == 1 ? "it" : "one of them")."))
                }
                .font(.ui(.callout))
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

/// Gives every navigation stack in this window's tab pages `top` points more safe area,
/// so each starts below the floating top bar yet scrolls on beneath it, and `bottom` more
/// for the floating tab bar. Set in UIKit
/// because a stack ignores SwiftUI's `safeAreaPadding` and `safeAreaInset` from outside
/// it. Only the hierarchy's children are touched, never a presented sheet or cover, which
/// have no top bar over them. Re-made per tab (`.id`), since each tab brings its own
/// stack, and it looks again shortly after, since a stack can appear after this does.
private struct StackInset: UIViewRepresentable {
    let top: CGFloat
    let bottom: CGFloat

    func makeUIView(context: Context) -> Probe { Probe() }

    func updateUIView(_ probe: Probe, context: Context) {
        probe.top = top
        probe.bottom = bottom
        probe.apply()
    }

    final class Probe: UIView {
        var top: CGFloat = 0
        var bottom: CGFloat = 0

        override func didMoveToWindow() {
            super.didMoveToWindow()
            apply()
            for delay in [0.05, 0.3, 1.0] {
                DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in self?.apply() }
            }
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            apply()
        }

        /// Sets the insets only when they really change. A measured height jitters by a
        /// fraction of a point from one layout pass to the next; setting the stacks'
        /// insets for that lays the whole page out again, which measures again. On a
        /// phone with a large library that loop froze Home's scrolling for seconds at a
        /// time (UIKit-runloop hang reports, 2026-11-22).
        func apply() {
            guard let root = window?.rootViewController else { return }
            let scale = window?.screen.scale ?? 3
            let top = (self.top * scale).rounded() / scale
            let bottom = (self.bottom * scale).rounded() / scale
            func walk(_ vc: UIViewController) {
                if let stack = vc as? UINavigationController {
                    var insets = stack.additionalSafeAreaInsets
                    if abs(insets.top - top) >= 0.5 || abs(insets.bottom - bottom) >= 0.5 {
                        insets.top = top
                        insets.bottom = bottom
                        stack.additionalSafeAreaInsets = insets
                    }
                }
                vc.children.forEach(walk)
            }
            walk(root)
        }
    }
}
