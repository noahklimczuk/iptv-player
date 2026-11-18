import SwiftUI

/// Something to play full screen.
enum PlayerItem: Identifiable, Hashable {
    case channel(id: Int64, name: String)
    case movie(id: Int64, title: String, startAt: Double?)
    case episode(id: Int64, title: String, startAt: Double?)
    /// A programme that already aired on a channel, `start..stop` in Unix seconds.
    case catchup(channelId: Int64, title: String, start: Int64, stop: Int64)

    var id: String {
        switch self {
        case .channel(let id, _): "channel-\(id)"
        case .movie(let id, _, _): "movie-\(id)"
        case .episode(let id, _, _): "episode-\(id)"
        case .catchup(let id, _, let start, _): "catchup-\(id)-\(start)"
        }
    }

    var title: String {
        switch self {
        case .channel(_, let name): name
        case .movie(_, let title, _), .episode(_, let title, _), .catchup(_, let title, _, _): title
        }
    }
}

/// Full-screen playback.
///
/// Siri Remote, controls hidden: left and right skip back 10 and forward 30 seconds on
/// films and episodes, up and down change channel on live TV, a click shows the controls, Play/Pause pauses. With the
/// controls up the remote moves between them, and Menu closes a panel, then cancels
/// Up Next, then hides the controls, then closes the player. On iPhone and iPad a tap shows or hides the controls.
struct PlayerScreen: View {
    @Environment(AppModel.self) private var model
    let item: PlayerItem

    var body: some View {
        if let core = model.core {
            PlayerContent(item: item, core: core, profileId: model.profileId)
        }
    }
}

private struct PlayerContent: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var session: PlayerSession
    @State private var controller: MPVPlayerController?
    /// The tune and the item it is for: Up Next swaps the item in place.
    @State private var resolved: (item: PlayerItem, tune: Resolved)?
    @State private var startedItem: PlayerItem?
    @FocusState private var focus: PlayerFocus?

    private enum Resolved {
        case live(LiveTune)
        case vod(VodTune)
        case replay(CatchupTune)
    }

    init(item: PlayerItem, core: AuroraCore, profileId: Int64) {
        _session = State(initialValue: PlayerSession(item: item, core: core, profileId: profileId))
    }

    var body: some View {
        ZStack {
            MPVPlayerView(
                controller: $controller,
                onStatus: { session.status = $0 },
                onState: { session.update($0) },
                onEnded: { if !session.ended() { dismiss() } }
            )
            .ignoresSafeArea()
            backdrop
            PlayerOverlay(session: session, focus: $focus, close: { dismiss() })
                #if os(tvOS)
                .onExitCommand(perform: menu)
                #endif
        }
        .task(id: session.item) { await resolve(session.item) }
        // Whichever arrives second — the tune or the player view — starts playback.
        .onChange(of: controller != nil) { playIfReady() }
        .onChange(of: resolved?.item) { playIfReady() }
        .onChange(of: session.sleptAt) { if session.sleptAt != nil { dismiss() } }
        .onAppear {
            session.start()
            focus = .playPause
        }
        .onDisappear {
            session.stop()
            controller?.stop()
            Task { await model.reload() }
        }
        #if os(tvOS)
        .onPlayPauseCommand { session.togglePause() }
        #endif
    }

    #if os(tvOS)
    /// Menu on the remote: a panel closes, then Up Next is cancelled, then the controls
    /// go, then the player. Attached beside the focusable views themselves: one
    /// on the screen as a whole never hears it, the full-screen cover closes first.
    private func menu() {
        if session.panel != nil {
            session.panel = nil
        } else if focus == .upNextPlay || focus == .upNextCancel {
            // Up Next is the one thing on screen asking something: Menu says no.
            session.dismissUpNext()
        } else if session.controlsVisible {
            session.hideControls()
        } else {
            dismiss()
        }
    }
    #endif

    /// Under the controls: on tvOS what the remote talks to while they are hidden, on
    /// iPhone and iPad what a tap on the picture lands on.
    @ViewBuilder private var backdrop: some View {
        #if os(tvOS)
        if !session.controlsVisible && session.panel == nil {
            Button { session.poke() } label: {
                Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .buttonStyle(BareButtonStyle())
            // Under Skip and Up Next while one of them has focus, so moving between
            // their buttons can't fall through to the picture.
            .disabled(focus?.isCorner == true)
            .focused($focus, equals: .backdrop)
            .onExitCommand(perform: menu)
            .onMoveCommand { direction in
                switch direction {
                case .left: session.nudge(by: -10)
                case .right: session.nudge(by: 30)
                // On a channel, up and down change channel, as on any TV.
                case .up where session.isLive: session.step(forward: true)
                case .down where session.isLive: session.step(forward: false)
                default: session.poke()
                }
            }
            .ignoresSafeArea()
        }
        #else
        Color.clear
            .contentShape(Rectangle())
            .onTapGesture {
                if session.panel != nil { session.panel = nil } else { session.toggleControls() }
            }
            .ignoresSafeArea()
        #endif
    }

    private func resolve(_ item: PlayerItem) async {
        let core = session.core
        do {
            let tune = try await Task.detached { () throws -> Resolved in
                switch item {
                case .channel(let id, _): .live(try core.tuneLive(channelId: id))
                case .movie(let id, _, let startAt): .vod(try core.tuneMovie(movieId: id, startAtSecs: startAt))
                case .episode(let id, _, let startAt): .vod(try core.tuneEpisode(episodeId: id, startAtSecs: startAt))
                case .catchup(let id, _, let start, let stop):
                    .replay(try core.tuneCatchup(channelId: id, start: start, stop: stop))
                }
            }.value
            guard item == session.item else { return }
            resolved = (item, tune)
        } catch {
            // A channel zapped past while it was still resolving isn't the one failing.
            guard item == session.item else { return }
            session.status = .failed(error.viewerMessage)
        }
    }

    private func playIfReady() {
        guard let resolved, resolved.item != startedItem, let controller else { return }
        startedItem = resolved.item
        session.controller = controller
        let core = session.core
        switch resolved.tune {
        case .live(let tune): controller.play(tune, core: core)
        case .vod(let tune): controller.play(tune, core: core, profileId: model.profileId)
        case .replay(let tune): controller.play(tune, core: core)
        }
        // The picture's shape is the viewer's, not the file's: it carries to the next one.
        if session.aspect != .auto { controller.setAspect(session.aspect) }
    }
}

/// Hosts the UIKit player controller in SwiftUI.
struct MPVPlayerView: UIViewControllerRepresentable {
    @Binding var controller: MPVPlayerController?
    var onStatus: (MPVPlayerController.PlayerStatus) -> Void
    var onState: (MPVPlayerController.PlaybackState) -> Void
    var onEnded: () -> Void

    func makeUIViewController(context: Context) -> MPVPlayerController {
        let player = MPVPlayerController()
        updateUIViewController(player, context: context)
        DispatchQueue.main.async { controller = player }
        return player
    }

    func updateUIViewController(_ player: MPVPlayerController, context: Context) {
        player.onStatus = onStatus
        player.onState = onState
        player.onEnded = onEnded
    }
}
