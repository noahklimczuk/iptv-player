import SwiftUI

/// Multi-view: choosing what to open (Windows' `MosaicPage`, README §7.4).
///
/// The arrangement, a channel per tile, saved layouts, and the one thing this screen
/// exists to say before anything is opened: whether the provider's line can carry it.
/// The words and the order are Windows'; picking a channel is a searchable list rather
/// than Windows' dropdown, which a remote cannot type into.
struct MultiViewPage: View {
    @Environment(AppModel.self) private var model

    /// Windows' `LAYOUTS`, in its order: cheapest line first.
    static let layouts: [(id: String, label: String, hint: String, tiles: Int)] = [
        ("grid2x2", "2 × 2", "Four equal tiles", 4),
        ("onePlusThree", "1 + 3", "One large, three beside it", 4),
        ("onePlusFive", "1 + 5", "One large, five around it", 6),
        ("grid3x3", "3 × 3", "Nine equal tiles", 9),
    ]

    @State private var layout = "grid2x2"
    @State private var picked: [Int64?] = []
    @State private var channels: [Channel] = []
    @State private var loaded = false
    @State private var check: MosaicCheck?
    @State private var saved: [SavedMosaicLayout] = []
    @State private var savedVersion = 0
    @State private var confirming: Int64?
    @FocusState private var savedFocus: SavedFocus?
    @State private var opening = false
    @State private var choosingTile: Int?
    @State private var session: MosaicSession?
    @State private var playing: PlayerItem?
    @State private var afterWall: PlayerItem?
    @State private var failure: (title: String, message: String)?

    private var tiles: Int { Self.layouts.first { $0.id == layout }?.tiles ?? 4 }
    private var filled: Int { picked.prefix(tiles).compactMap { $0 }.count }
    /// What *this* selection costs: an empty tile opens no connection.
    private var selectionBlocked: Bool {
        guard let check, check.verdict == "exceeds", let limit = check.limit else { return false }
        return filled + Int(check.recordings) > Int(limit)
    }

    var body: some View {
        NavigationStack {
            Group {
                if loaded && channels.isEmpty {
                    ContentUnavailableView(
                        "No channels to show", systemImage: "square.grid.2x2",
                        description: Text("Multi-view needs live channels. Add a provider in Settings and refresh your library."))
                } else {
                    ScrollView { content.padding(pagePadding) }
                }
            }
            .auroraTitle("Multi-view")
        }
        .task(id: layout) {
            let name = layout
            check = await (try? model.guarded { try $0.mosaicCheck(layout: name) })
        }
        .task(id: model.recordingsVersion) {
            let name = layout
            check = await (try? model.guarded { try $0.mosaicCheck(layout: name) })
        }
        .task(id: savedVersion) {
            saved = await (try? model.guarded { try $0.mosaicLayouts() }) ?? []
        }
        .task(id: model.libraryVersion) {
            channels = Self.ordered(await model.channels(group: nil, limit: 100_000))
            loaded = true
        }
        // Choosing a smaller arrangement drops the tiles that no longer exist rather than
        // keeping them invisibly.
        .onChange(of: layout) { picked = Array(picked.prefix(tiles)) }
        .sheet(item: Binding(get: { choosingTile.map(ChosenTile.init) }, set: { choosingTile = $0?.index })) { chosen in
            ChannelChooser(
                title: "Tile \(chosen.index + 1)\(chosen.index == 0 ? " — starts with sound" : "")",
                channels: channels,
                selected: picked.indices.contains(chosen.index) ? picked[chosen.index] : nil
            ) { id in
                setTile(chosen.index, id)
                choosingTile = nil
            }
            .auroraAppearance()
        }
        .fullScreenCover(item: $session, onDismiss: {
            if let item = afterWall { afterWall = nil; playing = item }
        }) { wall in
            MosaicScreen(session: wall, promote: { item in
                afterWall = item
                session = nil
            }, saved: { savedVersion += 1 })
        }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
        .alert(failure?.title ?? "", isPresented: Binding(
            get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("OK") { failure = nil }
        } message: {
            Text(failure?.message ?? "")
        }
    }

    private var pagePadding: CGFloat {
        #if os(tvOS)
        0
        #else
        20
        #endif
    }

    @ViewBuilder private var content: some View {
        VStack(alignment: .leading, spacing: 20) {
            PageHeading("Multi-view")
            #if os(tvOS)
            let how = "Watch several channels at once. One tile has sound — select a tile to move it, and hold it to open that channel on its own."
            #else
            let how = "Watch several channels at once. One tile has sound — tap a tile to move it, and double-tap to open that channel on its own."
            #endif
            Text(how).foregroundStyle(.secondary).fontWeight(.medium)

            Picker("Arrangement", selection: $layout) {
                ForEach(Self.layouts, id: \.id) { l in
                    Text(l.label).tag(l.id)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityHint(Self.layouts.first { $0.id == layout }.map { "\($0.hint) · \($0.tiles) tiles" } ?? "")

            if let check { budget(check) }

            Text("Channels — leave a tile empty to open fewer streams")
                .font(.subheadline.bold()).foregroundStyle(.secondary)
            wall

            HStack(spacing: 12) {
                Button(opening ? "Opening…" : "Open \(filled) channel\(filled == 1 ? "" : "s")",
                       systemImage: "square.grid.2x2") { open(rotating: false) }
                    .buttonStyle(.borderedProminent)
                    .disabled(filled == 0 || selectionBlocked || opening)
                if filled == 0 {
                    Text("Pick at least one channel.").font(.footnote).foregroundStyle(.secondary)
                }
                if selectionBlocked, let check {
                    Text("\(filled + Int(check.recordings)) streams is more than your provider allows.")
                        .font(.footnote).foregroundStyle(.red)
                }
            }

            // The way out, where there used to be none: offered only when the
            // alternative is nothing at all.
            if selectionBlocked, check?.rotationOffered == true, filled > 1 {
                VStack(alignment: .leading, spacing: 8) {
                    Label("Show them one at a time instead", systemImage: "square.stack.3d.up")
                        .font(.subheadline.bold())
                    Text("Your line allows one stream, so Aurora can move it around the wall — each channel for \(dwell) seconds, with the rest showing what is on now. \(stayHint)")
                        .font(.footnote).foregroundStyle(.secondary)
                    Button(opening ? "Opening…" : "Rotate \(filled) channel\(filled == 1 ? "" : "s")",
                           systemImage: "square.stack.3d.up") { open(rotating: true) }
                        .buttonStyle(.borderedProminent)
                        .disabled(opening)
                }
                .padding()
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 20))
            }

            if !saved.isEmpty { savedLayouts }
        }
    }

    #if os(tvOS)
    private let stayHint = "Select a tile to stay on it."
    #else
    private let stayHint = "Tap a tile to stay on it."
    #endif

    private var dwell: UInt32 { model.core?.mosaicDwellSecs() ?? 12 }

    // MARK: The budget line

    @ViewBuilder private func budget(_ check: MosaicCheck) -> some View {
        let blocked = check.verdict == "exceeds"
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: blocked ? "exclamationmark.triangle.fill" : "info.circle")
            Text(Self.budgetLine(check) + (blocked ? " " + Self.fitting(check) : ""))
        }
        .font(.footnote.weight(.semibold))
        .foregroundStyle(blocked ? Color.red : Color.secondary)
        .padding(.horizontal, 14).padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(blocked ? Color.red.opacity(0.12) : Color.secondary.opacity(0.08),
                    in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12)
            .strokeBorder(blocked ? Color.red.opacity(0.4) : Color.secondary.opacity(0.25)))
    }

    /// Windows' `budgetLine`.
    static func budgetLine(_ check: MosaicCheck) -> String {
        switch check.verdict {
        case "exceeds":
            let r = check.recordings
            let recordings = r > 0 ? " (\(r) recording\(r == 1 ? "" : "s") using the line)" : ""
            return "Needs \(check.needed), your provider allows \(check.limit ?? 0)\(recordings)"
        case "unknown":
            return "Needs \(check.needed) streams — your provider has not said how many it allows"
        default:
            return "\(check.needed) of \(check.limit ?? 0) connections"
        }
    }

    static func fitting(_ check: MosaicCheck) -> String {
        if let largest = check.largestFitting {
            let label = layouts.first { $0.id == largest }?.label ?? largest
            return "— the largest that fits is \(label)."
        }
        return "— no arrangement fits on this line, so multi-view cannot be used with this provider."
    }

    // MARK: The wall as it will open

    private var wall: some View {
        let columns = Array(repeating: GridItem(.flexible(), spacing: 16), count: tiles >= 9 ? 3 : 2)
        return LazyVGrid(columns: columns, spacing: 16) {
            ForEach(0..<tiles, id: \.self) { index in
                let id = picked.indices.contains(index) ? picked[index] : nil
                let channel = id.flatMap { id in channels.first { $0.id == id } }
                Button { choosingTile = index } label: {
                    TileCard(index: index, name: channel?.name, short: tiles >= 9)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Tile \(index + 1)\(index == 0 ? " — starts with sound" : ""): \(channel?.name ?? "Empty")")
            }
        }
        .padding(16)
        .background(.background.tertiary, in: RoundedRectangle(cornerRadius: 28))
    }

    private func setTile(_ index: Int, _ id: Int64?) {
        var next = picked
        while next.count < tiles { next.append(nil) }
        next[index] = id
        picked = next
    }

    // MARK: Saved layouts

    private enum SavedFocus: Hashable { case ask(Int64), keep(Int64) }

    private var savedLayouts: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Saved layouts").font(.title2.bold())
            ForEach(saved, id: \.id) { row in
                HStack(spacing: 12) {
                    LayoutThumb(layout: row.layout)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.name).fontWeight(.semibold)
                        Text("\(row.channels.compactMap { $0 }.count) channels")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Open", systemImage: "play.fill") { openSaved(row) }
                    // Asks first: a saved layout exists nowhere else, and there is no undo.
                    if confirming == row.id {
                        Text("Delete this layout?").font(.footnote).foregroundStyle(.red)
                        Button("Delete", role: .destructive) {
                            confirming = nil
                            delete(row)
                        }
                        .buttonStyle(.borderedProminent)
                        Button("Keep") {
                            confirming = nil
                            // Back to the button that asked, rather than the top of the page.
                            Task { savedFocus = .ask(row.id) }
                        }
                        .focused($savedFocus, equals: .keep(row.id))
                    } else {
                        Button("Delete \(row.name)", systemImage: "xmark") {
                            confirming = row.id
                            // The safe answer has focus, so a second press keeps it.
                            Task { savedFocus = .keep(row.id) }
                        }
                        .labelStyle(.iconOnly)
                        .focused($savedFocus, equals: .ask(row.id))
                    }
                }
                .buttonStyle(.bordered)
                .padding(.horizontal, 16).padding(.vertical, 12)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 16))
            }
        }
    }

    // MARK: Opening

    private func open(rotating: Bool) {
        let layout = layout, wanted = Array(picked.prefix(tiles)) + Array(repeating: nil, count: max(0, tiles - picked.count))
        start(layout: layout, channels: wanted, rotating: rotating, failing: "Could not open multi-view")
    }

    private func openSaved(_ row: SavedMosaicLayout) {
        start(layout: row.layout, channels: row.channels, rotating: false, failing: "Could not open \(row.name)")
    }

    /// Windows' `open_sharing`: refuse what the line cannot carry, plan every tile, and
    /// put the wall up. A tile that could not be planned says why rather than failing
    /// the whole mosaic: it is one stream of several.
    private func start(layout: String, channels wanted: [Int64?], rotating: Bool, failing: String) {
        guard let core = model.core else { return }
        opening = true
        Task {
            defer { opening = false }
            do {
                let filled = wanted.compactMap { $0 }.count
                try await model.guarded { try $0.mosaicAdmit(streams: UInt32(rotating ? min(filled, 1) : filled)) }
                var planned: [MosaicSession.Tile] = []
                for (index, id) in wanted.enumerated() {
                    var tile = MosaicSession.Tile(index: index, channelId: id)
                    if let id {
                        tile.name = channels.first { $0.id == id }?.name
                        do {
                            let tune = try await model.guarded { try $0.tuneTile(channelId: id) }
                            tile.tune = tune
                            tile.name = tune.title
                        } catch {
                            tile.error = error.viewerMessage
                        }
                    }
                    planned.append(tile)
                }
                session = MosaicSession(layout: layout, tiles: planned, rotating: rotating,
                                        dwell: core.mosaicDwellSecs(), core: core)
            } catch where error.wantsParentalPin {
                // Asked for the PIN and the viewer said never mind.
            } catch {
                failure = (failing, error.viewerMessage)
            }
        }
    }

    private func delete(_ row: SavedMosaicLayout) {
        Task {
            do {
                _ = try await model.guarded { try $0.mosaicDeleteLayout(id: row.id) }
                savedVersion += 1
            } catch {
                failure = ("Could not delete \(row.name)", error.viewerMessage)
            }
        }
    }

    /// Favourites first, then by the provider's group, number and name (Windows'
    /// `channelOptions`).
    static func ordered(_ all: [Channel]) -> [Channel] {
        let favourites = all.filter(\.favorite)
        let rest = all.filter { !$0.favorite }.sorted {
            let (a, b) = ($0.group ?? "", $1.group ?? "")
            if a != b { return a.localizedCompare(b) == .orderedAscending }
            if ($0.number ?? 0) != ($1.number ?? 0) { return ($0.number ?? 0) < ($1.number ?? 0) }
            return $0.name.localizedCompare($1.name) == .orderedAscending
        }
        return favourites + rest
    }
}

private struct ChosenTile: Identifiable {
    let index: Int
    var id: Int { index }
}

/// One tile of the wall before it opens: a dashed "Add a channel", or the channel on
/// its hue, the sound tile ringed.
private struct TileCard: View {
    let index: Int
    let name: String?
    let short: Bool

    var body: some View {
        VStack(alignment: .leading) {
            HStack(spacing: 8) {
                if let name {
                    Text("\(index + 1)").opacity(0.8)
                    Text(name).lineLimit(1)
                    Spacer(minLength: 0)
                    if index == 0 { Image(systemName: "speaker.wave.2.fill").font(.footnote) }
                } else {
                    Label("Add a channel", systemImage: "plus").foregroundStyle(.secondary)
                    Spacer(minLength: 0)
                }
            }
            .font(.headline.weight(.heavy))
            Spacer(minLength: 0)
            Text(name == nil ? "Empty" : (index == 0 ? "Starts with sound" : "Tile \(index + 1)"))
                .font(.caption).opacity(0.75)
        }
        .padding(14)
        .frame(maxWidth: .infinity, minHeight: short ? 120 : 150, alignment: .topLeading)
        .foregroundStyle(name == nil ? Color.primary : .white)
        .background {
            if let name { RoundedRectangle(cornerRadius: 22).fill(ChannelTile.hue(name)) }
        }
        .overlay {
            if name == nil {
                RoundedRectangle(cornerRadius: 22)
                    .strokeBorder(style: StrokeStyle(lineWidth: 2, dash: [6])).foregroundStyle(.tertiary)
            } else if index == 0 {
                RoundedRectangle(cornerRadius: 22).strokeBorder(Color.accentColor, lineWidth: 4)
            }
        }
        .contentShape(RoundedRectangle(cornerRadius: 22))
    }
}

/// A miniature of an arrangement, from the core's own geometry.
struct LayoutThumb: View {
    @Environment(AppModel.self) private var model
    let layout: String
    var small = false

    var body: some View {
        let size = small ? CGSize(width: 24, height: 16) : CGSize(width: 64, height: 44)
        let rects = (try? model.core?.mosaicRects(layout: layout, width: UInt32(size.width * 10),
                                                  height: UInt32(size.height * 10))) ?? []
        ZStack(alignment: .topLeading) {
            ForEach(Array(rects.enumerated()), id: \.offset) { _, r in
                RoundedRectangle(cornerRadius: 2)
                    .fill(Color.primary.opacity(0.24))
                    .frame(width: max(0, CGFloat(r.width) / 10 - 2), height: max(0, CGFloat(r.height) / 10 - 2))
                    .offset(x: CGFloat(r.x) / 10 + 1, y: CGFloat(r.y) / 10 + 1)
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .accessibilityHidden(true)
    }
}

/// Choose a tile's channel: Favourites first, then the provider's groups, searchable by
/// name or group, as Windows' channel `Select` is.
private struct ChannelChooser: View {
    @Environment(\.dismiss) private var dismiss
    let title: String
    let channels: [Channel]
    let selected: Int64?
    let choose: (Int64?) -> Void
    @State private var query = ""

    var body: some View {
        NavigationStack {
            List {
                Button { choose(nil) } label: {
                    HStack {
                        Text("Empty").foregroundStyle(.secondary)
                        Spacer()
                        if selected == nil { Image(systemName: "checkmark") }
                    }
                }
                ForEach(sections, id: \.name) { section in
                    Section(section.name) {
                        ForEach(section.channels, id: \.id) { channel in
                            Button { choose(channel.id) } label: {
                                HStack {
                                    Text(channel.name)
                                    Spacer()
                                    if let n = channel.number { Text("\(n)").foregroundStyle(.secondary) }
                                    if selected == channel.id { Image(systemName: "checkmark") }
                                }
                            }
                        }
                    }
                }
            }
            .searchable(text: $query)
            .auroraPageTitle(title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            #endif
        }
    }

    /// Grouped as Windows' `Select` heads them; the filter matches the group's name too,
    /// so "sport" finds the sports channels whatever they are called.
    private var sections: [(name: String, channels: [Channel])] {
        let q = query.trimmingCharacters(in: .whitespaces)
        var out: [(name: String, channels: [Channel])] = []
        for channel in channels {
            let group = channel.favorite ? "Favourites" : (channel.group ?? "Everything else")
            if !q.isEmpty && !channel.name.localizedCaseInsensitiveContains(q)
                && !group.localizedCaseInsensitiveContains(q) { continue }
            if out.last?.name == group { out[out.count - 1].channels.append(channel) }
            else { out.append((group, [channel])) }
        }
        return out
    }
}
