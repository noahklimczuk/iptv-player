import SwiftUI

/// An open multi-view wall (README §7.4): Windows' host `Mosaic` session and its
/// `MosaicOverlay`, together.
///
/// On Windows the host keeps the session because it owns the tiles' windows; here each
/// tile is an mpv view this app owns, so the session lives here and asks the core only
/// for what has to match Windows: the tiles' geometry, their tunes, the budget, saved
/// layouts. The rules are Windows': exactly one tile has sound; a rotation hands its
/// one stream on every `dwell` seconds, sound following the picture; choosing a tile in
/// a rotation brings the stream to it and stays there until Resume.
@MainActor @Observable
final class MosaicSession: Identifiable {
    struct Tile: Identifiable {
        let index: Int
        var channelId: Int64?
        var name: String?
        /// What to play, planned at open: nil for an empty tile or one that could not be.
        var tune: LiveTune?
        var error: String?
        var playing = false
        /// Whether this tile is holding a stream now: every planned tile in an ordinary
        /// mosaic, only the showing one in a rotation.
        var streaming = false
        /// Bumped for each new stream, so the tile's player is built afresh: the old one
        /// lets its connection go as the new one asks.
        var generation = 0
        var id: Int { index }
    }

    struct Rotation {
        var showing: Int
        var since: Date
        /// Set while the viewer has pinned a tile by choosing it.
        var pinned: Bool
    }

    let id = UUID()
    let layout: String
    private(set) var tiles: [Tile]
    private(set) var focused = 0
    private(set) var rotation: Rotation?
    /// Ticks once a second, for the countdown and the rotation.
    private(set) var now = Date()
    let dwell: TimeInterval
    private let core: AuroraCore
    private var players: [Int: MPVPlayerController] = [:]

    init(layout: String, tiles: [Tile], rotating: Bool, dwell: UInt32, core: AuroraCore) {
        self.layout = layout
        self.tiles = tiles
        self.dwell = TimeInterval(dwell)
        self.core = core
        let first = tiles.firstIndex { $0.tune != nil } ?? 0
        focused = first
        if rotating {
            rotation = Rotation(showing: first, since: Date(), pinned: false)
            if tiles.indices.contains(first), tiles[first].tune != nil { self.tiles[first].streaming = true }
        } else {
            for i in self.tiles.indices where self.tiles[i].tune != nil { self.tiles[i].streaming = true }
        }
    }

    var rotating: Bool { rotation != nil }

    /// Seconds until the picture moves on, or nil: an ordinary mosaic, or a pinned one.
    var nextInSecs: Int? {
        guard let rotation, !rotation.pinned else { return nil }
        return max(0, Int((rotation.since.addingTimeInterval(dwell).timeIntervalSince(now)).rounded(.up)))
    }

    /// A tile's player is up: start its channel, muted unless it is the tile with sound.
    func attach(_ player: MPVPlayerController, to index: Int) {
        guard let tune = tiles[index].tune else { return }
        players[index] = player
        player.play(tune, core: core)
        player.setMuted(index != focused)
    }

    func detach(_ player: MPVPlayerController, from index: Int) {
        if players[index] === player { players[index] = nil }
    }

    func status(_ status: MPVPlayerController.PlayerStatus, of index: Int) {
        switch status {
        case .opening: tiles[index].playing = false
        case .playing, .noPicture:
            tiles[index].playing = true
            tiles[index].error = nil
        case .failed(let why):
            tiles[index].playing = false
            tiles[index].error = why
        }
        applyAudio()
    }

    /// Move sound to a tile, or in a rotation bring the picture to it and stay there.
    /// Throws Windows' words when there is nothing there to move to.
    func focus(_ index: Int) throws {
        guard tiles.indices.contains(index) else {
            throw MosaicRefusal("There is no tile \(index + 1) in this layout.")
        }
        if var rotation {
            guard tiles[index].tune != nil else {
                throw MosaicRefusal("That tile has no channel in it to show.")
            }
            if rotation.showing != index { hand(from: rotation.showing, to: index) }
            rotation = Rotation(showing: index, since: Date(), pinned: true)
            self.rotation = rotation
            focused = index
            applyAudio()
            return
        }
        guard tiles[index].streaming else {
            throw MosaicRefusal("That tile has no picture, so there is no sound to move to it.")
        }
        focused = index
        applyAudio()
    }

    /// Let a rotation the viewer pinned start moving again.
    func resumeRotation() {
        guard rotation != nil else { return }
        rotation?.pinned = false
        rotation?.since = Date()
    }

    /// Once a second while the wall is up: the clock, and whose turn it is.
    func tick() {
        now = Date()
        guard let r = rotation, !r.pinned, now.timeIntervalSince(r.since) >= dwell,
              let next = nextInRotation(from: r.showing) else { return }
        hand(from: r.showing, to: next)
        rotation = Rotation(showing: next, since: now, pinned: false)
        // Sound follows the picture. On one stream they cannot be apart.
        focused = next
        applyAudio()
    }

    /// Stop every tile. Their views let go of the players as the wall goes away.
    func close() {
        for player in players.values { player.stop() }
        players.removeAll()
        for i in tiles.indices { tiles[i].streaming = false }
    }

    /// The channels as they stand, tile by tile, for "Save layout".
    var channels: [Int64?] { tiles.map(\.channelId) }

    /// One stream, so the tile being left lets go before the tile chosen asks.
    private func hand(from: Int, to: Int) {
        players[from]?.stop()
        players[from] = nil
        tiles[from].streaming = false
        tiles[from].playing = false
        // Not broken, simply not on any more.
        tiles[from].error = nil
        tiles[to].streaming = true
        tiles[to].generation += 1
    }

    /// The next tile with something to show, wrapping; nil when only this one has.
    private func nextInRotation(from: Int) -> Int? {
        let n = tiles.count
        return (1...n).map { (from + $0) % n }.first { $0 != from && tiles[$0].tune != nil }
    }

    /// Exactly one tile audible, and only if it has a stream.
    private func applyAudio() {
        for (index, player) in players { player.setMuted(index != focused) }
    }
}

struct MosaicRefusal: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

/// The wall itself: every tile's picture where the core's geometry puts it, with
/// Windows' chrome over each, and the bar at the top.
struct MosaicScreen: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let session: MosaicSession
    /// Close the wall and open this tile's channel in the player.
    let promote: (PlayerItem) -> Void
    /// A layout was saved, so the list under the wall is out of date.
    let saved: () -> Void

    /// Read here as the player reads it: a full-screen cover is its own presentation.
    @AppStorage(AppearanceKey.theme) private var theme: AuroraTheme = .dark
    @State private var guide: [Int64: NowNext] = [:]
    @State private var failure: (title: String, message: String)?
    @State private var naming = false
    @State private var name = ""
    @FocusState private var focusedTile: Int?
    /// Which of the bar's buttons has focus, if one does (tvOS).
    @FocusState private var barItem: BarItem?
    private enum BarItem: Hashable { case resume, save, close }
    /// Focus is in the bar, so the tiles stand aside. Its own state rather than
    /// `barItem != nil`, which the focused button holds on to until focus has moved.
    @State private var inBar = false

    var body: some View {
        GeometryReader { geo in
            let rects = (try? model.core?.mosaicRects(
                layout: session.layout, width: UInt32(max(1, geo.size.width)),
                height: UInt32(max(1, geo.size.height)))) ?? []
            ZStack(alignment: .topLeading) {
                Color.black
                ForEach(session.tiles) { tile in
                    if rects.indices.contains(tile.index) {
                        let r = rects[tile.index]
                        tileView(tile, top: r.y == 0)
                            .frame(width: CGFloat(r.width), height: CGFloat(r.height))
                            .offset(x: CGFloat(r.x), y: CGFloat(r.y))
                    }
                }
                bar
                    .frame(maxWidth: .infinity, alignment: .trailing)
                    .padding(barPadding)
            }
        }
        .ignoresSafeArea()
        .environment(\.auroraTheme, theme)
        .preferredColorScheme(theme.scheme)
        .task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                session.tick()
            }
        }
        // Now and next for the tiles waiting their turn, once a minute.
        .task(id: session.rotating) {
            guard session.rotating else { return }
            let ids = session.tiles.compactMap(\.channelId)
            while !Task.isCancelled {
                let rows = await model.nowNext(channelIds: ids)
                guide = Dictionary(rows.map { ($0.channelId, $0) }, uniquingKeysWith: { a, _ in a })
                try? await Task.sleep(for: .seconds(60))
            }
        }
        .onDisappear { session.close() }
        .alert(failure?.title ?? "", isPresented: Binding(
            get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("OK") { failure = nil }
        } message: {
            Text(failure?.message ?? "")
        }
        .alert("Save this layout", isPresented: $naming) {
            TextField("Sunday football", text: $name)
            Button("Cancel", role: .cancel) { name = "" }
            Button("Save") { save() }
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
        } message: {
            Text("Name")
        }
        #if os(tvOS)
        .onExitCommand { dismiss() }
        #endif
    }

    private var barPadding: CGFloat {
        #if os(tvOS)
        40
        #else
        16
        #endif
    }

    // MARK: Tiles

    @ViewBuilder private func tileView(_ tile: MosaicSession.Tile, top: Bool) -> some View {
        let empty = tile.channelId == nil && tile.error == nil
        let waiting = session.rotating && !tile.streaming && tile.error == nil && tile.channelId != nil
        ZStack(alignment: .bottomLeading) {
            // The picture, or what stands in for it.
            if tile.streaming {
                MosaicTilePlayer(
                    onReady: { session.attach($0, to: tile.index) },
                    onGone: { session.detach($0, from: tile.index) },
                    onStatus: { session.status($0, of: tile.index) })
                    .id(tile.generation)
            }
            if empty {
                theme.surfaceSunk
                Text("Empty")
                    .font(.system(size: Daylight.px(13), weight: .bold))
                    .foregroundStyle(theme.textMuted)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = tile.error {
                theme.surfaceSunk
                VStack(spacing: Daylight.px(8)) {
                    Image(systemName: "exclamationmark.triangle").font(.system(size: Daylight.px(20)))
                    Text(error).font(.system(size: Daylight.px(12))).multilineTextAlignment(.center).opacity(0.85)
                }
                .padding(Daylight.px(12)).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if waiting {
                // Waiting its turn: the channel it stands for, and what is on it now and
                // next, which is why the viewer picked it. A shade off the player's black,
                // so the wall reads as cards and one picture rather than as failures.
                Color(white: 0.06)
                VStack(alignment: .leading, spacing: 4) {
                    Text(tile.name ?? "").font(.system(size: Daylight.px(13), weight: .semibold)).lineLimit(1)
                    if let id = tile.channelId, let now = guide[id]?.now {
                        Text(now.title).font(.system(size: Daylight.px(12))).opacity(0.85).lineLimit(1)
                    }
                    if let id = tile.channelId, let next = guide[id]?.next {
                        Text("Next: \(next.title)").font(.system(size: Daylight.px(11))).opacity(0.55).lineLimit(1)
                    }
                }
                .padding(Daylight.px(12)).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            } else if !tile.playing {
                Color.black.allowsHitTesting(false)
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            // The number is the shortcut, so it is on screen.
            HStack(spacing: 8) {
                Text("\(tile.index + 1)").monospacedDigit().fontWeight(.heavy)
                    .opacity(tile.index < 9 ? 1 : 0.4)
                Text(tile.name ?? "—").lineLimit(1)
                Spacer(minLength: 0)
                if tile.index == session.focused && !empty {
                    Image(systemName: "speaker.wave.2.fill").font(.caption)
                }
            }
            .font(.system(size: Daylight.px(13), weight: .semibold))
            .foregroundStyle(empty ? theme.text : .white)
            .padding(.horizontal, Daylight.px(14)).padding(.top, Daylight.px(18)).padding(.bottom, Daylight.px(10))
            .background {
                if !empty {
                    LinearGradient(colors: [Color(red: 10 / 255, green: 12 / 255, blue: 20 / 255).opacity(0.75), .clear],
                                   startPoint: .bottom, endPoint: .top)
                }
            }
        }
        .foregroundStyle(.white)
        .overlay {
            if empty {
                Rectangle().strokeBorder(theme.borderStrong, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
            }
        }
        // The sound tile carries the accent ring, drawn inside so it never covers a
        // neighbour.
        .overlay {
            if tile.index == session.focused && !empty {
                Rectangle().strokeBorder(theme.accent, lineWidth: 4)
            }
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label(tile))
        .accessibilityAddTraits(.isButton)
        #if os(tvOS)
        // Not while the bar has focus: the bar is drawn over the top row, and the focus
        // engine would otherwise take left or right in the bar back to the tile under it.
        .focusable(!inBar)
        .focused($focusedTile, equals: tile.index)
        .overlay {
            // Windows' focus ring, inside the tile so it never covers a neighbour.
            if focusedTile == tile.index {
                Rectangle().strokeBorder(theme.background, lineWidth: 2).padding(6)
                Rectangle().strokeBorder(Color.white, lineWidth: 3).padding(8)
            }
        }
        .onTapGesture { choose(tile.index) }
        // The bar is drawn over the top row, so the focus engine finds nothing above a
        // top tile: up from there goes to the bar.
        .onMoveCommand {
            guard $0 == .up && top else { return }
            inBar = true
            barItem = .save
        }
        .contextMenu {
            if tile.channelId != nil {
                Button("Open on its own", systemImage: "arrow.up.left.and.arrow.down.right") {
                    open(tile)
                }
            }
        }
        #else
        .onTapGesture(count: 2) { open(tile) }
        .onTapGesture { choose(tile.index) }
        #endif
    }

    private func label(_ tile: MosaicSession.Tile) -> String {
        guard let name = tile.name else { return "Tile \(tile.index + 1), empty" }
        var text = "Tile \(tile.index + 1): \(name)"
        if session.rotating && !tile.streaming { text += ", waiting its turn" }
        if tile.index == session.focused { text += ", has audio" }
        return text
    }

    private func choose(_ index: Int) {
        do {
            try session.focus(index)
        } catch {
            failure = ("Could not move to that tile", error.localizedDescription)
        }
    }

    private func open(_ tile: MosaicSession.Tile) {
        guard let id = tile.channelId else {
            failure = ("Could not open that channel on its own", "That tile has no channel in it.")
            return
        }
        promote(.channel(id: id, name: tile.name ?? ""))
    }

    // MARK: The bar

    /// Windows' bar over the wall: the dock's glass, its title in the display face, the
    /// rotation's countdown on a dock button's well, and Save layout and Close.
    private var bar: some View {
        let dock = DockTokens(theme)
        return HStack(spacing: Daylight.px(8)) {
            Text("Multi-view")
                .font(.system(size: Daylight.px(20), weight: .heavy))
                .padding(.leading, Daylight.px(8)).padding(.trailing, Daylight.px(10))
            if session.rotating {
                HStack(spacing: Daylight.px(8)) {
                    Image(systemName: "square.stack.3d.up").font(.system(size: Daylight.px(14)))
                    if let left = session.nextInSecs {
                        Text("Next tile in \(left)s").monospacedDigit()
                    } else {
                        Text("Staying on \(session.tiles[session.focused].name ?? "this tile")").lineLimit(1)
                        Button("Resume") { session.resumeRotation() }
                            .buttonStyle(.daylight(.secondary, .sm))
                            .focused($barItem, equals: .resume)
                            .modifier(BackToTiles(action: backToTiles))
                    }
                }
                .font(.system(size: Daylight.px(12), weight: .bold))
                .padding(.horizontal, Daylight.px(12))
                .frame(minHeight: Daylight.px(36))
                .background(dock.button, in: RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous))
            }
            Button("Save layout", systemImage: "plus") { naming = true }
                .focused($barItem, equals: .save)
                .modifier(BackToTiles(action: backToTiles))
            Button("Close", systemImage: "xmark") { dismiss() }
                .focused($barItem, equals: .close)
                .modifier(BackToTiles(action: backToTiles))
        }
        .buttonStyle(.daylight(.secondary))
        .foregroundStyle(dock.text)
        .padding(6)
        .dockGlass(RoundedRectangle(cornerRadius: Daylight.px(18), style: .continuous), dock)
    }

    /// Down from the bar: back to the tile with sound. The tiles take focus again only once
    /// the bar has let go, so the tile is focused on the next turn rather than this one.
    private func backToTiles() {
        inBar = false
        Task { focusedTile = session.focused }
    }

    private func save() {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        let layout = session.layout, channels = session.channels
        Task {
            do {
                _ = try await model.guarded { try $0.mosaicSave(name: trimmed, layout: layout, channels: channels) }
                name = ""
                saved()
            } catch {
                failure = ("Could not save the layout", error.viewerMessage)
            }
        }
    }
}

/// One tile's player. Built afresh for each stream (`Tile.generation`); taken down with
/// the tile, which stops its stream and so gives its connection back.
struct MosaicTilePlayer: UIViewControllerRepresentable {
    let onReady: (MPVPlayerController) -> Void
    let onGone: (MPVPlayerController) -> Void
    let onStatus: (MPVPlayerController.PlayerStatus) -> Void

    func makeUIViewController(context: Context) -> MPVPlayerController {
        let player = MPVPlayerController()
        player.onStatus = onStatus
        context.coordinator.onGone = onGone
        // After SwiftUI has loaded the view, which is when mpv exists.
        DispatchQueue.main.async { onReady(player) }
        return player
    }

    func updateUIViewController(_ player: MPVPlayerController, context: Context) {
        player.onStatus = onStatus
    }

    static func dismantleUIViewController(_ player: MPVPlayerController, coordinator: Coordinator) {
        player.stop()
        coordinator.onGone?(player)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {
        var onGone: ((MPVPlayerController) -> Void)?
    }
}

/// Down from a button in the wall's bar (tvOS): the focus engine finds nothing below it,
/// because the bar is drawn over the tiles and they stand aside while it has focus.
private struct BackToTiles: ViewModifier {
    let action: () -> Void
    func body(content: Content) -> some View {
        #if os(tvOS)
        content.onMoveCommand { if $0 == .down { action() } }
        #else
        content
        #endif
    }
}
