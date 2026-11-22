import SwiftUI

/// Home's billboard (Windows' `HeroBillboard`): six featured titles, one at a time, each
/// over its backdrop with Play, My List and More Info. It moves on every 12 seconds, and
/// holds while one of its buttons has the focus, as Windows holds under the pointer.
///
/// Not here: the trailer that Windows plays over the backdrop after two seconds. It is a
/// YouTube embed, which tvOS cannot show (apple/README.md).
struct HeroBillboard: View {
    let items: [CatalogItem]
    let open: (CatalogItem) -> Void

    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    @AppStorage(AppearanceKey.animations) private var animations = true
    @State private var index = 0
    @State private var onMyList: Set<String> = []
    @State private var playing: PlayerItem?
    @FocusState private var focused: Control?

    private enum Control: Hashable { case play, myList, info }

    private static let rotateEvery: Duration = .seconds(12)

    var body: some View {
        if let item = items[safe: index] ?? items.first {
            ZStack(alignment: .bottomLeading) {
                artwork(item)
                    .id(item.key)
                    .transition(.opacity)
                scrims
                text(item)
                    .padding(EdgeInsets(top: inset, leading: inset, bottom: inset, trailing: inset))
                    .frame(maxWidth: textWidth, alignment: .leading)
            }
            .overlay(alignment: .topTrailing) { dots.padding(inset - 4) }
            .frame(maxWidth: .infinity, minHeight: minHeight, maxHeight: .infinity)
            .background(Self.art)
            .clipShape(RoundedRectangle(cornerRadius: 24))
            .environment(\.colorScheme, .dark)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Featured")
            .task(id: RotateKey(index: index, held: focused != nil, animations: animations)) {
                guard animations, focused == nil, items.count > 1 else { return }
                try? await Task.sleep(for: Self.rotateEvery)
                guard !Task.isCancelled else { return }
                withAnimation(.easeInOut(duration: 0.5)) { index = (index + 1) % items.count }
            }
            .task(id: item.key) {
                if let marks = await model.marks(item.listKind, id: item.id), marks.inMyList {
                    onMyList.insert(item.key)
                } else {
                    onMyList.remove(item.key)
                }
            }
            .onChange(of: items.map(\.key)) { index = 0 }
            .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
        }
    }

    // MARK: Artwork

    /// The backdrop, or the poster blown out behind glass when there is none, or
    /// Windows' `--art-1` with its disc when there is neither.
    private func artwork(_ item: CatalogItem) -> some View {
        // A slow drift, so a title waiting its turn is not a screenshot.
        Drift(on: animations, art: art(item))
    }

    private func art(_ item: CatalogItem) -> some View {
        GeometryReader { box in
            Group {
                if let backdrop = item.backdrop.flatMap(URL.init(string:)) {
                    AsyncImage(url: backdrop) { $0.resizable().scaledToFill() } placeholder: { Color.clear }
                } else if let poster = item.poster.flatMap(URL.init(string:)) {
                    AsyncImage(url: poster) { image in
                        image.resizable().scaledToFill()
                            .blur(radius: 28)
                            .saturation(1.25)
                            .brightness(-0.28)
                            .scaleEffect(1.15)
                    } placeholder: { Color.clear }
                } else {
                    Circle()
                        .fill(.white.opacity(0.14))
                        .frame(width: box.size.width * 0.92)
                        .position(x: box.size.width * 0.78, y: box.size.height * 0.18)
                }
            }
            .frame(width: box.size.width, height: box.size.height)
        }
    }

    /// `--scrim` and `--scrim-side`: always ink, so white type reads on any backdrop.
    private var scrims: some View {
        let ink = theme == .oled || theme == .contrast
            ? Color.black : Color(red: 22 / 255, green: 22 / 255, blue: 26 / 255)
        return ZStack {
            LinearGradient(stops: [.init(color: ink.opacity(0.82), location: 0),
                                   .init(color: ink.opacity(0.45), location: 0.35),
                                   .init(color: .clear, location: 0.7)],
                           startPoint: .bottom, endPoint: .top)
            LinearGradient(stops: [.init(color: ink.opacity(0.85), location: 0),
                                   .init(color: ink.opacity(0.5), location: 0.4),
                                   .init(color: .clear, location: 0.8)],
                           startPoint: .leading, endPoint: .trailing)
        }
        .allowsHitTesting(false)
    }

    // MARK: Text and buttons

    private func text(_ item: CatalogItem) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(item.isSeries ? "Featured series" : "Featured film")
                .font(.ui(.caption).weight(.heavy))
                .foregroundStyle(Color(red: 22 / 255, green: 22 / 255, blue: 26 / 255))
                .padding(.horizontal, 10).padding(.vertical, 5)
                .background(.white, in: Capsule())
                .padding(.bottom, 14)
            Text(item.title)
                .displayFont(size: titleSize)
                .foregroundStyle(.white)
                .lineLimit(2)
                .padding(.bottom, 10)
            meta(item)
                .font(.ui(.subheadline).weight(.bold))
                .foregroundStyle(.white.opacity(0.88))
                .padding(.bottom, 12)
            if let overview = item.overview, !overview.isEmpty {
                Text(overview)
                    .foregroundStyle(.white.opacity(0.85))
                    .lineLimit(2)
                    .frame(maxWidth: overviewWidth, alignment: .leading)
                    .padding(.bottom, 20)
            }
            // Not under a white foreground: a focused TV button turns white, and its
            // label has to go dark.
            buttons(item)
        }
        .id(item.key)
        .transition(.opacity)
    }

    /// "2019 · 1h 52m", or "2019 4 seasons", as Windows' meta line.
    private func meta(_ item: CatalogItem) -> some View {
        HStack(spacing: 8) {
            switch item {
            case .movie(let movie):
                if let year = movie.year { Text(String(year)) }
                let length = runtime(movie.runtimeMins)
                if !length.isEmpty { Text("· \(length)") }
            case .series(let series):
                if let year = series.year { Text(String(year)) }
                if !series.seasons.isEmpty {
                    Text("\(series.seasons.count) season\(series.seasons.count == 1 ? "" : "s")")
                }
            }
        }
    }

    @ViewBuilder private func buttons(_ item: CatalogItem) -> some View {
        #if os(tvOS)
        buttonRow(item, iconsOnly: false)
            .focusSection()
            // Coming in from the tabs or the rails, Play first, wherever the remote
            // entered: `defaultFocus` does not steer a move into the section on tvOS.
            .onChange(of: focused) { old, new in
                if old == nil, let new, new != .play { focused = .play }
            }
        #else
        // On a phone the three labels do not fit across: My List and More Info keep
        // their icons and give up their words, which VoiceOver still reads.
        ViewThatFits(in: .horizontal) {
            buttonRow(item, iconsOnly: false)
            buttonRow(item, iconsOnly: true)
        }
        #endif
    }

    private func buttonRow(_ item: CatalogItem, iconsOnly: Bool) -> some View {
        let listed = onMyList.contains(item.key)
        return HStack(spacing: buttonSpacing) {
            Button { Task { await play(item) } } label: {
                Label("Play", systemImage: "play.fill")
            }
            .focused($focused, equals: .play)
            #if !os(tvOS)
            .buttonStyle(SolidOnArt())
            #endif
            Button {
                Task {
                    if let now = await model.toggleMyList(item.listKind, id: item.id) {
                        if now { onMyList.insert(item.key) } else { onMyList.remove(item.key) }
                    }
                }
            } label: {
                Label("My List", systemImage: listed ? "checkmark" : "plus")
                    .labelStyle(iconOnly: iconsOnly)
            }
            .accessibilityAddTraits(listed ? .isSelected : [])
            .focused($focused, equals: .myList)
            #if !os(tvOS)
            .buttonStyle(GlassOnArt())
            #endif
            Button { open(item) } label: {
                Label("More Info", systemImage: "info.circle")
                    .labelStyle(iconOnly: iconsOnly)
            }
            .focused($focused, equals: .info)
            #if !os(tvOS)
            .buttonStyle(GlassOnArt())
            #endif
        }
    }

    /// One pill per title, the current one long. Pressable where there is a pointer or a
    /// finger; on a TV they only say where the rotation is.
    private var dots: some View {
        HStack(spacing: 5) {
            ForEach(Array(items.enumerated()), id: \.element.key) { offset, entry in
                let current = offset == index
                Capsule()
                    .fill(.white.opacity(current ? 1 : 0.4))
                    .frame(width: current ? 22 : 8, height: 8)
                    .animation(animations ? .easeInOut(duration: 0.2) : nil, value: index)
                    #if !os(tvOS)
                    .padding(.vertical, 8)
                    .contentShape(Rectangle())
                    .onTapGesture { withAnimation(.easeInOut(duration: 0.5)) { index = offset } }
                    .accessibilityLabel("Show \(entry.title)")
                    .accessibilityAddTraits(.isButton)
                    #endif
            }
        }
        #if os(tvOS)
        .accessibilityHidden(true)
        #endif
    }

    // MARK: Playing

    /// A film from where it was left; a show from the episode left unfinished or the first
    /// unwatched one, else its first episode (Windows' `play`).
    private func play(_ item: CatalogItem) async {
        switch item {
        case .movie(let movie):
            let progress = await model.progress(.movie, id: movie.id)
            let resume = progress.flatMap { $0.completed || $0.positionSecs <= 0 ? nil : Double($0.positionSecs) }
            playing = .movie(id: movie.id, title: movie.title, startAt: resume)
        case .series(let series):
            let episodes = await model.episodes(seriesId: series.id, season: nil)
            let resume = await model.seriesResume(seriesId: series.id)
            guard let episode = episodes.first(where: { $0.id == resume?.episodeId }) ?? episodes.first else {
                model.error = "No episodes of \(series.title) have been imported yet"
                return
            }
            let startAt = resume.flatMap { $0.episodeId == episode.id && $0.positionSecs > 0 ? Double($0.positionSecs) : nil }
            playing = .episode(id: episode.id, title: Self.label(episode), startAt: startAt)
        }
    }

    static func label(_ e: Episode) -> String {
        let number = "S\(e.season)E\(e.episode)"
        guard let title = e.title, !title.isEmpty else { return number }
        return "\(number) · \(title)"
    }

    // MARK: Picking

    /// A seed fixed for the run of the app: the billboard is different each time Aurora
    /// is opened, and stays put while it is open (Windows' `heroSeed`).
    private static let sessionSeed = UInt32(truncatingIfNeeded: Int(Date().timeIntervalSince1970 * 1000))

    /// Windows' choice: the head of the rails about the viewer and about what is popular
    /// first, the rest of the screen as a fallback; nothing they are part-way through or
    /// already put on My List; titles with a backdrop when there are six of them; a
    /// seeded shuffle; six.
    static func pick(from rails: [Rail]) -> [CatalogItem] {
        let skip: Set<String> = ["continueWatching", "upNext", "myList"]
        let decided = Set(rails.filter { skip.contains($0.kind) }.flatMap { $0.items.map(\.item.key) })
        let lead: Set<String> = ["forYou", "top10", "trending", "becauseYouWatched"]
        let ordered = rails.filter { lead.contains($0.kind) }.map { ($0.kind, Array($0.items.prefix(8))) }
            + rails.filter { !lead.contains($0.kind) }.map { ($0.kind, $0.items) }
        var seen = Set<String>()
        var pool: [CatalogItem] = []
        for (kind, items) in ordered where !skip.contains(kind) {
            for entry in items {
                let key = entry.item.key
                guard !decided.contains(key), seen.insert(key).inserted else { continue }
                pool.append(entry.item)
            }
        }
        let withArt = pool.filter { $0.backdrop != nil }
        var chosen = withArt.count >= 6 ? withArt : pool
        // Mulberry32, as Windows: small, and good enough to pick six films.
        var state = sessionSeed
        func random() -> Double {
            state = state &+ 0x6D2B_79F5
            var t = state
            t = (t ^ (t >> 15)) &* (t | 1)
            t ^= t &+ ((t ^ (t >> 7)) &* (t | 61))
            return Double(t ^ (t >> 14)) / 4_294_967_296
        }
        if chosen.count > 1 {
            for i in stride(from: chosen.count - 1, to: 0, by: -1) {
                let j = Int(random() * Double(i + 1))
                chosen.swapAt(i, j)
            }
        }
        return Array(chosen.prefix(6))
    }

    private struct RotateKey: Equatable {
        let index: Int
        let held: Bool
        let animations: Bool
    }

    /// `--art-1`: what the billboard is when a title has no artwork at all.
    static let art = Color(red: 0x2B / 255, green: 0x4B / 255, blue: 1)

    #if os(tvOS)
    private let minHeight: CGFloat = 560
    private let inset: CGFloat = 48
    private let titleSize: CGFloat = 60
    private let textWidth: CGFloat = 900
    private let overviewWidth: CGFloat = 760
    private let buttonSpacing: CGFloat = 12
    #else
    private let buttonSpacing: CGFloat = 10
    private let minHeight: CGFloat = 400
    private let inset: CGFloat = 28
    private let titleSize: CGFloat = 34
    private let textWidth: CGFloat = 640
    private let overviewWidth: CGFloat = 520
    #endif
}

#if !os(tvOS)
/// Play over artwork: white, with ink type (`ON_ART_SOLID`).
private struct SolidOnArt: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.ui(.headline).weight(.heavy))
            .lineLimit(1).fixedSize()
            .foregroundStyle(Color(red: 22 / 255, green: 22 / 255, blue: 26 / 255))
            .padding(.horizontal, 20).padding(.vertical, 12)
            .background(.white, in: Capsule())
            .opacity(configuration.isPressed ? 0.8 : 1)
    }
}

/// My List and More Info over artwork: clear glass, white type (`ON_ART_GLASS`).
private struct GlassOnArt: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.ui(.headline).weight(.heavy))
            .lineLimit(1).fixedSize()
            .foregroundStyle(.white)
            .padding(.horizontal, 18).padding(.vertical, 12)
            .glassEffect(.clear.interactive(), in: Capsule())
            .opacity(configuration.isPressed ? 0.8 : 1)
    }
}
#endif

/// Windows' `runtime`: "1h 52m", "48m", or nothing when unknown.
func runtime(_ mins: UInt32?) -> String {
    guard let mins, mins > 0 else { return "" }
    let h = mins / 60, m = mins % 60
    return h > 0 ? "\(h)h \(m)m" : "\(m)m"
}

/// Home's first row, as Windows' bento: the billboard in seven columns of twelve and
/// "On now" in the other five, both as tall as the taller.
struct BentoRow: Layout {
    var spacing: CGFloat

    private func widths(_ total: CGFloat) -> (CGFloat, CGFloat) {
        let lead = ((total - spacing) * 7 / 12).rounded()
        return (lead, total - spacing - lead)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 1200
        let (a, b) = widths(width)
        let heights = zip(subviews, [a, b]).map { $0.sizeThatFits(.init(width: $1, height: nil)).height }
        return CGSize(width: width, height: heights.max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let (a, b) = widths(bounds.width)
        guard subviews.count == 2 else { return }
        subviews[0].place(at: bounds.origin, proposal: .init(width: a, height: bounds.height))
        subviews[1].place(at: CGPoint(x: bounds.minX + a + spacing, y: bounds.minY),
                          proposal: .init(width: b, height: bounds.height))
    }
}

/// Icon only or icon and title, chosen at run time. The style is applied to the view
/// rather than wrapped, because calling a system style's `makeBody` by hand reads its
/// environment outside a view (Xcode's "Accessing Environment" warning).
private extension View {
    @ViewBuilder func labelStyle(iconOnly on: Bool) -> some View {
        if on { labelStyle(.iconOnly) } else { labelStyle(.titleAndIcon) }
    }
}

extension CatalogItem {
    var isSeries: Bool { if case .series = self { true } else { false } }

    var backdrop: String? {
        switch self {
        case .movie(let m): m.backdrop
        case .series(let s): s.backdrop
        }
    }

    var overview: String? {
        switch self {
        case .movie(let m): m.overview
        case .series(let s): s.overview
        }
    }
}

extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}

/// The billboard's slow drift (Windows' 24-second scale to 1.08), run by Core Animation.
/// As a SwiftUI animation it was worked out on the main thread every frame, for as long
/// as the billboard existed, scrolled out of sight or not: on an iPhone that update was
/// a quarter of the time Home's scrolling stalled for (hang reports, 2026-11-22). Here
/// the artwork is drawn in a small host of its own, whose layer the render server
/// scales; the main thread never hears of it. The artwork reads nothing from the
/// environment, so it loses nothing by having its own.
private struct Drift<Art: View>: UIViewRepresentable {
    let on: Bool
    let art: Art

    func makeUIView(context: Context) -> Host { Host(UIHostingController(rootView: art)) }

    func updateUIView(_ host: Host, context: Context) {
        host.controller.rootView = art
        host.on = on
    }

    /// Whatever it is offered, as the `GeometryReader` it holds would take: in Home's
    /// scroll view no height is offered, and a UIKit view left to size itself there
    /// came out as tall as it liked, which broke the page.
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: Host, context: Context) -> CGSize? {
        proposal.replacingUnspecifiedDimensions()
    }

    final class Host: UIView {
        let controller: UIHostingController<Art>
        var on = true { didSet { if on != oldValue { start() } } }

        init(_ controller: UIHostingController<Art>) {
            self.controller = controller
            super.init(frame: .zero)
            clipsToBounds = true
            controller.view.backgroundColor = .clear
            controller.safeAreaRegions = []
            addSubview(controller.view)
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            // Bounds and centre, not frame: the drift scales this view's layer.
            controller.view.bounds = CGRect(origin: .zero, size: bounds.size)
            controller.view.center = CGPoint(x: bounds.midX, y: bounds.midY)
        }

        @available(*, unavailable) required init?(coder: NSCoder) { fatalError() }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            start()
        }

        private func start() {
            let layer = controller.view.layer
            layer.removeAnimation(forKey: "drift")
            guard on, window != nil else { return }
            let drift = CABasicAnimation(keyPath: "transform.scale")
            drift.fromValue = 1
            drift.toValue = 1.08
            drift.duration = 24
            drift.timingFunction = CAMediaTimingFunction(name: .linear)
            drift.fillMode = .forwards
            drift.isRemovedOnCompletion = false
            layer.add(drift, forKey: "drift")
        }
    }
}
