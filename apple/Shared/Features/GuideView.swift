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
    /// The clock the grid draws "on now" and the now line from. Ticked, because a TV is
    /// often left on the guide, and both would otherwise stay where they were drawn.
    @State private var now = Date()
    /// Which groups and kinds of channel are shown (`GuideFiltersView.swift`), kept on
    /// the device between visits.
    @State private var filters = GuideFilterStore.read()
    /// How many channels the filters leave, all pages together.
    @State private var shown: UInt32 = 0
    @State private var filtering = false

    #if os(tvOS)
    private let span: TimeInterval = 3 * 3600
    private let nameWidth: CGFloat = 320
    private let rowHeight: CGFloat = 96
    #else
    private let span: TimeInterval = 2 * 3600
    private let nameWidth: CGFloat = 110
    private let rowHeight: CGFloat = 64
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
                TimeAxis(start: start, span: span, leading: nameWidth)
                ScrollView {
                    LazyVStack(spacing: 4) {
                        ForEach(channels, id: \.id) { channel in
                            GuideRowView(channel: channel, cells: loader.cells[channel.id],
                                         window: window, now: now, nameWidth: nameWidth,
                                         height: rowHeight) { item in
                                playing = item
                            }
                            .onAppear {
                                loader.appeared(channel.id)
                                if channel.id == channels.last?.id { Task { await loadMoreChannels() } }
                            }
                            .onDisappear { loader.disappeared(channel.id) }
                        }
                    }
                }
                .overlay {
                    if channels.isEmpty {
                        if let progress = model.importProgress {
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
            .navigationTitle("Guide")
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
            .sheet(isPresented: $filtering) {
                GuideFiltersSheet(groups: model.groups, filters: filterBinding, shown: shown)
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
                Toggle("With listings", isOn: filterBinding.withListings)
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
    }

    private var controls: some View {
        HStack {
            Button("Earlier", systemImage: "chevron.backward") { start.addTimeInterval(-3600) }
            Button("Now") { start = Self.windowStart(for: Date()) }
            Button("Later", systemImage: "chevron.forward") { start.addTimeInterval(3600) }
            Spacer()
            Text(start, format: .dateTime.weekday(.wide).day().month())
                .foregroundStyle(.secondary)
        }
        .buttonStyle(.bordered)
        .padding(.horizontal)
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

private struct TimeAxis: View {
    let start: Date
    let span: TimeInterval
    let leading: CGFloat

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width - leading
            ForEach(0..<Int(span / 1800), id: \.self) { i in
                Text(start.addingTimeInterval(Double(i) * 1800), format: .dateTime.hour().minute())
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .offset(x: leading + width * CGFloat(Double(i) * 1800 / span))
            }
        }
        .frame(height: 24)
        .padding(.horizontal)
    }
}

private struct GuideRowView: View {
    let channel: Channel
    let cells: [GuideCell]?
    let window: GuideLoader.Window
    let now: Date
    let nameWidth: CGFloat
    let height: CGFloat
    let play: (PlayerItem) -> Void

    var body: some View {
        HStack(spacing: 0) {
            Button { play(.channel(id: channel.id, name: channel.name)) } label: {
                Text(channel.name).lineLimit(2).frame(width: nameWidth - 12, alignment: .leading)
            }
            .buttonStyle(.plain)
            .padding(.trailing, 12)
            GeometryReader { geo in
                ZStack(alignment: .topLeading) {
                    if let cells {
                        ForEach(cells, id: \.programme.id) { cell in
                            cellView(cell, width: geo.size.width)
                        }
                        if cells.isEmpty {
                            Text("No guide").font(.caption).foregroundStyle(.tertiary)
                                .frame(maxHeight: .infinity)
                        }
                    } else {
                        RoundedRectangle(cornerRadius: 8).fill(.quaternary.opacity(0.4))
                    }
                    nowLine(width: geo.size.width)
                }
            }
        }
        .frame(height: height)
        .padding(.horizontal)
    }

    private func cellView(_ cell: GuideCell, width: CGFloat) -> some View {
        let p = cell.programme
        let span = CGFloat(window.to - window.from)
        let from = CGFloat(max(p.start, window.from) - window.from)
        let to = CGFloat(min(p.stop, window.to) - window.from)
        let nowSecs = Int64(now.timeIntervalSince1970)
        let onNow = p.start <= nowSecs && nowSecs < p.stop
        return Button {
            if onNow {
                play(.channel(id: channel.id, name: channel.name))
            } else if cell.replayable {
                play(.catchup(channelId: channel.id, title: p.title, start: p.start, stop: p.stop))
            }
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 4) {
                    if cell.replayable { Image(systemName: "gobackward").font(.caption2) }
                    Text(p.title).font(.callout).lineLimit(1)
                }
                Text(Date(timeIntervalSince1970: TimeInterval(p.start)), format: .dateTime.hour().minute())
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 8)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .background(onNow ? AnyShapeStyle(.tint.opacity(0.25)) : AnyShapeStyle(.quaternary.opacity(0.5)),
                        in: RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        // Aired and not archived, or still to come: shown, but there's nothing to play.
        .disabled(!onNow && !cell.replayable)
        .frame(width: max(width * (to - from) / span - 3, 1))
        .offset(x: width * from / span)
        .accessibilityLabel("\(p.title), \(onNow ? "on now" : cell.replayable ? "can be replayed" : "")")
    }

    private func nowLine(width: CGFloat) -> some View {
        let nowSecs = now.timeIntervalSince1970
        let x = CGFloat((nowSecs - Double(window.from)) / Double(window.to - window.from))
        return Rectangle().fill(.red).frame(width: 2)
            .offset(x: width * x)
            .opacity((0...1).contains(x) ? 1 : 0)
            .allowsHitTesting(false)
    }
}
