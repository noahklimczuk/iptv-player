import SwiftUI

/// The programme guide: channels down, time across.
///
/// It fetches only the rows on screen, as Windows' `useGuideSlice.ts` does after R-130
/// (asking for every channel was 14.8 MB per step there). What it has learned is true of
/// one time window only, so moving the window starts afresh, and a reply for a window
/// that is no longer shown is dropped (R-91).
struct GuideView: View {
    @Environment(AppModel.self) private var model
    @State private var channels: [Channel] = []
    @State private var loader = GuideLoader()
    @State private var start = GuideView.windowStart(for: Date())
    @State private var playing: PlayerItem?
    /// The programme whose details are up (Windows' info pane), and what it asked to
    /// play: the player opens once the sheet has gone.
    @State private var chosen: ChosenProgramme?
    @State private var afterSheet: PlayerItem?
    /// The clock the grid draws "on now" and the now line from. Ticked, because a TV is
    /// often left on the guide, and both would otherwise stay where they were drawn.
    @State private var now = Date()
    /// Which groups and kinds of channel are shown (`GuideFiltersView.swift`), kept on
    /// the device between visits.
    @State private var filters = GuideFilterStore.read()
    /// How many channels the filters leave, all pages together.
    @State private var shown: UInt32 = 0
    /// The first page arrived: until then an empty list means "not read yet", not "no
    /// channels", and a big library takes a few seconds on a TV.
    @State private var loaded = false
    @State private var filtering = false

    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    private let span: TimeInterval = 3 * 3600
    /// Windows' TV-density `--guide-channel-w` and `--guide-row-h`.
    private let nameWidth: CGFloat = 300
    private let rowHeight: CGFloat = 80
    private let narrow = false
    #else
    @Environment(\.horizontalSizeClass) private var sizeClass
    private let span: TimeInterval = 2 * 3600
    /// Windows' `CHANNEL_W` and `ROW_H`; a phone keeps the logo and name only.
    private var narrow: Bool { sizeClass == .compact }
    private var nameWidth: CGFloat { narrow ? 150 : 264 }
    private let rowHeight: CGFloat = 56
    #endif

    private var window: GuideLoader.Window {
        .init(from: Int64(start.timeIntervalSince1970), to: Int64(start.addingTimeInterval(span).timeIntervalSince1970),
              library: model.libraryVersion)
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 8) {
                controls
                filterBar
                // Windows puts the grid in one white rounded panel (`dl-panel`).
                VStack(spacing: 0) {
                    TimeAxis(start: start, span: span, leading: nameWidth)
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(channels, id: \.id) { channel in
                                GuideRowView(channel: channel, cells: loader.cells[channel.id],
                                             window: window, now: now, nameWidth: nameWidth,
                                             height: rowHeight, narrow: narrow, marks: model.dvrMarks) { item in
                                    playing = item
                                } choose: { chosen = $0 }
                                .onAppear {
                                    loader.appeared(channel.id)
                                    if channel.id == channels.last?.id { Task { await loadMoreChannels() } }
                                }
                                .onDisappear { loader.disappeared(channel.id) }
                            }
                        }
                        #if os(tvOS)
                        .padding(.vertical, 12)
                        #endif
                    }
                    #if os(tvOS)
                    .scrollClipDisabled()
                    #endif
                }
                .padding(.vertical, 8)
                .daylightRaised(theme, radius: Daylight.px(24))
                .clipShape(RoundedRectangle(cornerRadius: Daylight.px(24), style: .continuous))
                .padding(.horizontal)
                .overlay {
                    if channels.isEmpty {
                        if !loaded {
                            VStack(spacing: 8) {
                                ForEach(0..<10, id: \.self) { _ in
                                    RoundedRectangle(cornerRadius: Daylight.px(14)).fill(theme.surfaceSunk)
                                        .frame(height: rowHeight - 8)
                                }
                            }
                            .padding(Daylight.px(24))
                            .frame(maxHeight: .infinity, alignment: .top)
                            .padding(.horizontal)
                        } else if let progress = model.importProgress {
                            ProgressView("Importing: \(progress.phase.label)")
                        } else if filters.activeCount > 0 {
                            ContentUnavailableView(
                                filters.withListings ? "No channels with listings here" : "No channels match these filters",
                                systemImage: "line.3.horizontal.decrease")
                        } else {
                            ContentUnavailableView("No channels", systemImage: "calendar",
                                                   description: Text("Add a provider in Live TV."))
                        }
                    }
                }
            }
            .auroraTitle("Guide")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .task(id: ListKey(library: model.libraryVersion, profile: model.profileId, filters: filters)) {
                await loadChannels()
            }
            .task(id: window) { await loader.show(window, model: model) }
            .task {
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(30))
                    now = Date()
                }
            }
            .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
            .programmeDetail(item: $chosen, onDismiss: {
                if let item = afterSheet { afterSheet = nil; playing = item }
            }) { c in
                ProgrammeDetail(channel: c.channel, programme: c.programme, replayable: c.replayable) { item in
                    afterSheet = item
                    chosen = nil
                }
            }
            .task(id: model.recordingsVersion) { await model.loadDvrMarks() }
            .sheet(isPresented: $filtering) {
                GuideFiltersSheet(groups: model.groups, filters: filterBinding, shown: shown)
                    .auroraAppearance()
            }
        }
    }

    /// What the list is read for: a new import, another profile's favourites, or another
    /// choice of filters each start it over.
    private struct ListKey: Hashable {
        let library: Int
        let profile: Int64
        let filters: GuideFilters
    }

    private var filterBinding: Binding<GuideFilters> {
        Binding(get: { filters }, set: { filters = $0; GuideFilterStore.write($0) })
    }

    /// Groups as chips rather than a menu, as on Windows: browsing is choosing between
    /// them, and a list you have to open to see is a list nobody reads. Each chip adds its
    /// group to the choice or takes it out, so several can be on at once; Filters has
    /// the rest — hiding groups instead, searching them, and the channel filters.
    private var filterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack {
                GuideFiltersButton(count: filters.activeCount) { filtering = true }
                FilterChip(title: "All channels", selected: filters.groups.isEmpty) {
                    filterBinding.wrappedValue.groups = []
                }
                ForEach(model.groups, id: \.name) { group in
                    FilterChip(title: group.name, selected: filters.groups.contains(group.name),
                               count: group.count, struck: filters.groupMode == .hide) {
                        filterBinding.wrappedValue = filters.toggling(group.name)
                    }
                }
                // Windows' `guide-listings`: the switch, then its words.
                HStack(spacing: 10) {
                    DaylightSwitch(isOn: filters.withListings, label: "Only channels with listings") {
                        filterBinding.wrappedValue.withListings.toggle()
                    }
                    Text("With listings")
                        .font(.ui(size: Daylight.px(13), weight: .bold))
                        .foregroundStyle(theme.textMuted)
                        .accessibilityHidden(true)
                }
                .fixedSize()
                .padding(.leading, 8)
            }
            .padding(.horizontal)
            #if os(tvOS)
            .padding(.vertical, 12)
            #endif
        }
        .scrollClipDisabled()
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private func loadChannels() async {
        let page = await model.guideChannels(filters, limit: 200)
        channels = page.channels
        shown = page.shown
        loaded = true
    }

    /// Windows' `GuideToolbar` top line: the day, then −24h −1h Now +1h +24h, then
    /// Tonight 8pm. On a phone the buttons go under the day and scroll sideways.
    private var controls: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) {
                dayHeading
                Spacer(minLength: 12)
                shiftButtons
                primeTimeButton
            }
            VStack(alignment: .leading, spacing: 8) {
                dayHeading
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 12) {
                        shiftButtons
                        primeTimeButton
                    }
                }
                .scrollClipDisabled()
            }
        }
        #if os(tvOS)
        .focusSection()
        #endif
        .padding(.horizontal)
    }

    private var dayHeading: some View {
        Text(Self.dayLabel(start))
            .displayFont(.title2)
            .lineLimit(1)
    }

    private var shiftButtons: some View {
        ActionSegments(items: [
            .init(title: "−24h") { start.addTimeInterval(-24 * 3600) },
            .init(title: "−1h") { start.addTimeInterval(-3600) },
            .init(title: "Now", systemImage: "clock", active: true) { start = Self.windowStart(for: Date()) },
            .init(title: "+1h") { start.addTimeInterval(3600) },
            .init(title: "+24h") { start.addTimeInterval(24 * 3600) },
        ])
    }

    /// Eight in the evening today, half an hour in, as Windows' `primeTime(0)`. Worded
    /// as Windows words it whatever the clock setting.
    private var primeTimeButton: some View {
        ActionSegments(items: [
            .init(title: "Tonight 8pm", systemImage: "moon") {
                let eight = Calendar.current.date(bySettingHour: 20, minute: 0, second: 0, of: Date()) ?? Date()
                start = eight.addingTimeInterval(-1800)
            },
        ])
    }

    /// Windows' `dayLabel`: Today, Tomorrow, Yesterday, else "Wed 19 Nov".
    static func dayLabel(_ date: Date, now: Date = Date()) -> String {
        let calendar = Calendar.current
        let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: now),
                                           to: calendar.startOfDay(for: date)).day ?? 0
        switch days {
        case 0: return "Today"
        case 1: return "Tomorrow"
        case -1: return "Yesterday"
        default: return date.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
        }
    }

    private func loadMoreChannels() async {
        guard channels.count < Int(shown) else { return }
        let asked = filters
        let page = await model.guideChannels(asked, limit: 200, offset: UInt32(channels.count))
        // The filters changed while this was in flight: the list has started over.
        guard asked == filters else { return }
        let known = Set(channels.map(\.id))
        channels += page.channels.filter { !known.contains($0.id) }
        shown = page.shown
    }

    /// The half hour before `date`, so "now" sits near the left edge.
    static func windowStart(for date: Date) -> Date {
        let t = date.timeIntervalSince1970
        return Date(timeIntervalSince1970: (t / 1800).rounded(.down) * 1800)
    }
}

/// What the guide has learned for the window on screen, and the rows still to ask for.
@MainActor
@Observable
final class GuideLoader {
    /// A time window over one state of the library: a new import is as much a new
    /// window as a new hour is, since what was learned may no longer be true.
    struct Window: Hashable, Sendable {
        let from: Int64
        let to: Int64
        let library: Int
    }

    /// Keyed by channel id. A channel with no guide has an empty array: asked, and the
    /// answer was nothing (most channels on a real panel), so it is not asked again.
    private(set) var cells: [Int64: [GuideCell]] = [:]
    private var window: Window?
    /// Rows on screen now: what a new window has to ask about again.
    private var visible: Set<Int64> = []
    private var asked: Set<Int64> = []
    private var pending: [Int64] = []
    private var settle: Task<Void, Never>?
    private weak var model: AppModel?

    /// How long scrolling has to stop before the rows on screen are fetched: Windows'
    /// `SETTLE_MS`, so a fast scroll is one request, not one per row passed.
    private static let settleTime: Duration = .milliseconds(120)

    func show(_ window: Window, model: AppModel) async {
        self.model = model
        guard window != self.window else { return }
        // A new window: nothing learned about the old one is true of this one.
        self.window = window
        cells = [:]
        asked = []
        pending = []
        for id in visible { want(id) }
    }

    func appeared(_ channelId: Int64) {
        visible.insert(channelId)
        want(channelId)
    }

    func disappeared(_ channelId: Int64) {
        visible.remove(channelId)
    }

    private func want(_ channelId: Int64) {
        guard !asked.contains(channelId), !pending.contains(channelId) else { return }
        pending.append(channelId)
        settle?.cancel()
        settle = Task { [weak self] in
            try? await Task.sleep(for: Self.settleTime)
            guard !Task.isCancelled else { return }
            await self?.fetch()
        }
    }

    private func fetch() async {
        guard let window, let model, !pending.isEmpty else { return }
        let ids = pending
        pending = []
        asked.formUnion(ids)
        // In chunks the core will answer in full (it caps one call at 120 rows).
        for start in stride(from: 0, to: ids.count, by: 100) {
            let chunk = Array(ids[start..<min(start + 100, ids.count)])
            let rows = await model.guideRows(channelIds: chunk, from: window.from, to: window.to)
            // The window may have moved while this was in flight: then this answer is
            // about a time no longer on screen, and the new window has already re-asked.
            guard window == self.window else { return }
            for row in rows { cells[row.channelId] = row.cells }
        }
    }
}

/// Windows' `TimeHeader`: CHANNEL over the channel column, then the half hours.
private struct TimeAxis: View {
    let start: Date
    let span: TimeInterval
    let leading: CGFloat
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        HStack(spacing: 0) {
            Text("CHANNEL")
                .font(.ui(size: Daylight.px(12), weight: .heavy))
                .tracking(Daylight.px(12) * 0.06)
                .foregroundStyle(theme.textFaint)
                .padding(.horizontal, 16)
                .frame(width: leading, alignment: .leading)
            GeometryReader { geo in
                ForEach(0..<Int(span / 1800), id: \.self) { i in
                    Text(start.addingTimeInterval(Double(i) * 1800), format: .dateTime.hour().minute())
                        .font(.ui(size: Daylight.px(15), weight: .semibold).monospacedDigit())
                        .foregroundStyle(theme.textMuted)
                        .padding(.horizontal, 8)
                        .frame(maxHeight: .infinity)
                        .offset(x: geo.size.width * CGFloat(Double(i) * 1800 / span))
                }
            }
        }
        .frame(height: Daylight.px(36))
    }
}

private struct GuideRowView: View {
    let channel: Channel
    let cells: [GuideCell]?
    let window: GuideLoader.Window
    let now: Date
    let nameWidth: CGFloat
    let height: CGFloat
    /// A phone: the channel column keeps the logo and name, not the number.
    let narrow: Bool
    let marks: DvrMarks
    let play: (PlayerItem) -> Void
    /// Open a programme's details (Windows: a click selects, the info pane shows it).
    let choose: (ChosenProgramme) -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        HStack(spacing: 0) {
            Button { play(.channel(id: channel.id, name: channel.name)) } label: { channelCell }
                .buttonStyle(GuideChannelStyle())
                .frame(width: nameWidth)
                .accessibilityLabel("Watch \(channel.name)")
            GeometryReader { geo in
                ZStack(alignment: .topLeading) {
                    if let cells {
                        if cells.isEmpty {
                            FromTheName(channel: channel)
                                .padding(EdgeInsets(top: 4, leading: 2, bottom: 4, trailing: 4))
                        }
                        ForEach(cells, id: \.programme.id) { cell in
                            cellView(cell, width: geo.size.width)
                        }
                    } else {
                        RoundedRectangle(cornerRadius: Daylight.px(14)).fill(theme.surfaceSunk.opacity(0.6))
                            .padding(4)
                    }
                    nowLine(width: geo.size.width)
                }
            }
        }
        .frame(height: height)
    }

    /// Windows' `ChannelCell`: number, logo, the name on two lines, and the catch-up mark.
    private var channelCell: some View {
        HStack(spacing: 10) {
            if !narrow {
                // Windows' 30-pixel column, which a five-digit number runs past rather
                // than wrapping.
                Text(verbatim: channel.number.map { String($0) } ?? "")
                    .font(.ui(size: Daylight.px(12), weight: .bold).monospacedDigit())
                    .foregroundStyle(theme.textFaint)
                    .lineLimit(1)
                    .fixedSize()
                    .frame(minWidth: Daylight.px(30), alignment: .trailing)
            }
            ChannelTile(name: channel.name, number: channel.number, logo: channel.logo,
                        size: narrow ? 32 : Daylight.px(40))
            Text(channel.name)
                .font(.ui(size: Daylight.px(13), weight: .heavy))
                .foregroundStyle(theme.text)
                .lineLimit(2)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            if channel.hasCatchup {
                Image(systemName: "gobackward.10")
                    .font(.ui(size: Daylight.px(13)))
                    .foregroundStyle(theme.textFaint)
                    .accessibilityLabel("Catch-up available")
            }
        }
        .padding(.leading, 8)
        .padding(.trailing, 12)
    }

    private func cellView(_ cell: GuideCell, width: CGFloat) -> some View {
        let p = cell.programme
        let span = CGFloat(window.to - window.from)
        let from = CGFloat(max(p.start, window.from) - window.from)
        let to = CGFloat(min(p.stop, window.to) - window.from)
        let nowSecs = Int64(now.timeIntervalSince1970)
        let airing = p.start <= nowSecs && nowSecs < p.stop
        let recording = marks.recording(channelId: channel.id, p) != nil
        let cellWidth = max(width * (to - from) / span, 2)
        let progress = airing ? Double(nowSecs - p.start) / Double(max(p.stop - p.start, 1)) : 0
        return Button {
            choose(ChosenProgramme(channel: channel, programme: p, replayable: cell.replayable))
        } label: {
            ProgrammeLabel(programme: p, recording: recording, replayable: cell.replayable,
                           showsTimes: cellWidth > Daylight.px(110))
        }
        .buttonStyle(GuideProgrammeStyle(airing: airing, past: p.stop < nowSecs, progress: progress))
        .frame(width: max(cellWidth - 4, 1), height: height - 8)
        .offset(x: width * from / span + 2, y: 4)
        .accessibilityLabel("\(p.title), \(airing ? "on now" : cell.replayable ? "can be replayed" : "")\(recording ? ", recording scheduled" : "")")
    }

    private func nowLine(width: CGFloat) -> some View {
        let nowSecs = now.timeIntervalSince1970
        let x = CGFloat((nowSecs - Double(window.from)) / Double(window.to - window.from))
        return Rectangle().fill(theme.live).frame(width: 2)
            .offset(x: width * x)
            .opacity((0...1).contains(x) ? 1 : 0)
            .allowsHitTesting(false)
    }
}

/// What a programme's pill says: the title with its marks, then its times when it is wide
/// enough to hold them (Windows: over 110 pixels).
private struct ProgrammeLabel: View {
    let programme: Programme
    let recording: Bool
    let replayable: Bool
    let showsTimes: Bool
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 5) {
                if recording {
                    Circle().fill(theme.live).frame(width: 7, height: 7)
                        .accessibilityLabel("Recording scheduled")
                }
                if programme.isNew { DaylightBadge("New", tone: .new, tight: true) }
                if programme.isLive { DaylightBadge("Live", tone: .live, tight: true) }
                Text(programme.title)
                    .font(.ui(size: Daylight.px(13), weight: .heavy))
                    .lineLimit(1)
            }
            if showsTimes {
                Text(verbatim: times)
                    .font(.ui(size: Daylight.px(12), weight: .bold))
                    .lineLimit(1)
                    .modifier(FaintUnlessChosen())
            }
        }
        .padding(.horizontal, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }

    private var times: String {
        var s = "\(clockTime(unix: programme.start))–\(clockTime(unix: programme.stop))"
        if let season = programme.season, let episode = programme.episode { s += " · S\(season)E\(episode)" }
        return s
    }
}

/// Whether the pill holding this is the chosen one, so its small print can follow it.
private struct ChosenKey: EnvironmentKey { static let defaultValue = false }
private extension EnvironmentValues {
    var guideChosen: Bool {
        get { self[ChosenKey.self] }
        set { self[ChosenKey.self] = newValue }
    }
}

private struct FaintUnlessChosen: ViewModifier {
    @Environment(\.guideChosen) private var chosen
    @Environment(\.auroraTheme) private var theme
    func body(content: Content) -> some View {
        content
            .foregroundStyle(chosen ? theme.accentText : theme.textFaint)
            .opacity(chosen ? 0.85 : 1)
    }
}

/// Windows' programme pill: sunk, the one on now tinted and ringed in the accent with its
/// progress along the foot, the past dimmed. The chosen one (on a TV, the focused one) is
/// the accent itself, ringed off the panel and glowing, so it is findable at a glance.
private struct GuideProgrammeStyle: ButtonStyle {
    let airing: Bool
    let past: Bool
    let progress: Double
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var chosen
    #else
    private let chosen = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let radius = Daylight.px(14)
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        configuration.label
            .foregroundStyle(chosen ? theme.accentText : theme.text)
            .environment(\.guideChosen, chosen)
            .background {
                shape.fill(chosen ? theme.accent : airing ? theme.accent.opacity(0.16) : .clear)
                    .background(chosen ? Color.clear : theme.surfaceSunk, in: shape)
            }
            .overlay(alignment: .bottomLeading) {
                if airing {
                    GeometryReader { geo in
                        Rectangle().fill(chosen ? theme.accentText : theme.accent)
                            .frame(width: geo.size.width * min(max(progress, 0), 1), height: 3)
                            .frame(maxHeight: .infinity, alignment: .bottom)
                    }
                }
            }
            .clipShape(shape)
            .overlay {
                if chosen {
                    // `0 0 0 2px var(--surface), 0 0 0 4px var(--accent)`.
                    RoundedRectangle(cornerRadius: radius + 4, style: .continuous)
                        .strokeBorder(theme.accent, lineWidth: 2).padding(-4)
                    RoundedRectangle(cornerRadius: radius + 2, style: .continuous)
                        .strokeBorder(theme.surface, lineWidth: 2).padding(-2)
                } else if airing {
                    shape.strokeBorder(theme.accent.opacity(0.45), lineWidth: 1.5)
                }
            }
            .shadow(color: theme.accent.opacity(chosen ? 0.45 : 0), radius: 10, y: 6)
            .opacity(past && !chosen ? 0.45 : 1)
            .opacity(configuration.isPressed ? 0.8 : 1)
            .zIndex(chosen ? 2 : 0)
            .contentShape(shape)
    }
}

/// Windows' `ChannelCell` ground: the panel's surface, and for the row you are on (on a
/// TV, the focused one) the accent tint with a bar down its edge.
private struct GuideChannelStyle: ButtonStyle {
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var active
    #else
    private let active = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let shape = UnevenRoundedRectangle(bottomTrailingRadius: Daylight.px(14),
                                           topTrailingRadius: Daylight.px(14), style: .continuous)
        configuration.label
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .background(active ? theme.accent.opacity(0.22) : .clear, in: shape)
            .overlay(alignment: .leading) {
                if active { Rectangle().fill(theme.accent).frame(width: 4) }
            }
            .contentShape(shape)
            .opacity(configuration.isPressed ? 0.75 : 1)
    }
}

/// Windows' `FromTheName`: a channel the guide knows nothing about, described from its
/// name in a dashed strip, so "nothing is known" never reads as "nothing is on".
private struct FromTheName: View {
    let channel: Channel
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous)
        // A phone's strip is too narrow for the tag as well: the title comes first.
        ViewThatFits(in: .horizontal) {
            strip(tagged: true)
            strip(tagged: false)
        }
        .font(.ui(size: Daylight.px(13)))
        .foregroundStyle(theme.textMuted)
        .padding(.horizontal, 14)
        .frame(maxWidth: Daylight.px(640), maxHeight: .infinity, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay { shape.strokeBorder(theme.borderStrong, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4])) }
        .accessibilityElement(children: .combine)
    }

    private func strip(tagged: Bool) -> some View {
        let g = NameGuide(channel.name)
        return HStack(spacing: 10) {
            Image(systemName: icon(g))
                .font(.ui(size: Daylight.px(14)))
                .opacity(0.7)
            Text(g.title)
                .font(.ui(size: Daylight.px(13), weight: plain(g) ? .bold : .heavy))
                .foregroundStyle(plain(g) ? theme.textMuted : theme.text)
                .lineLimit(1)
                // Whole beside the tag, or the tag gives way (see `body`).
                .fixedSize(horizontal: tagged, vertical: false)
            if let when = when(g) {
                Text(verbatim: when)
                    .font(.ui(size: Daylight.px(13), weight: .bold).monospacedDigit())
                    .fixedSize()
            }
            if tagged {
                Text("from channel name")
                    .font(.ui(size: Daylight.px(12), weight: .bold))
                    .foregroundStyle(theme.textFaint)
                    .padding(.vertical, 2).padding(.horizontal, 8)
                    .background(theme.surfaceSunk, in: Capsule())
                    .fixedSize()
            }
        }
    }

    private func plain(_ g: NameGuide) -> Bool {
        switch g {
        case .idle, .name: true
        default: false
        }
    }

    private func icon(_ g: NameGuide) -> String {
        switch g {
        case .event: "star"
        case .loop: "arrow.clockwise"
        default: "tv"
        }
    }

    private func when(_ g: NameGuide) -> String? {
        switch g {
        case .event(_, let at?, _):
            let date = Date(timeIntervalSince1970: TimeInterval(at))
            return "\(GuideView.dayLabel(date)) \(clockTime(date))"
        case .event(_, nil, let detail): return detail
        case .loop: return "All day"
        default: return nil
        }
    }
}
