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
/// Siri Remote: click or Play/Pause toggles pause, left/right skip on films and episodes,
/// Menu goes back. On iPhone and iPad a tap toggles pause and the close button goes back.
struct PlayerScreen: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let item: PlayerItem

    private enum Resolved {
        case live(LiveTune)
        case vod(VodTune)
        case replay(CatchupTune)
    }

    @State private var status: MPVPlayerController.PlayerStatus?
    @State private var controller: MPVPlayerController?
    @State private var resolved: Resolved?
    @State private var started = false

    var body: some View {
        ZStack(alignment: .topLeading) {
            MPVPlayerView(controller: $controller, onStatus: { status = $0 })
                .ignoresSafeArea()
            overlay
        }
        .task { await resolve() }
        // Whichever arrives second — the tune or the player view — starts playback.
        .onChange(of: controller != nil) { playIfReady() }
        .onChange(of: resolved != nil) { playIfReady() }
        .onDisappear {
            controller?.stop()
            Task { await model.reload() }
        }
        #if os(tvOS)
        .focusable()
        .onPlayPauseCommand { controller?.togglePause() }
        .onMoveCommand { direction in
            switch direction {
            case .left: controller?.seek(by: -10)
            case .right: controller?.seek(by: 30)
            default: break
            }
        }
        .onExitCommand { dismiss() }
        #else
        .onTapGesture { controller?.togglePause() }
        #endif
    }

    @ViewBuilder private var overlay: some View {
        switch status {
        case .opening(let source, let total):
            Label(total > 1 ? "Opening \(item.title) (source \(source) of \(total))" : "Opening \(item.title)",
                  systemImage: "dot.radiowaves.left.and.right")
                .padding()
                .background(.ultraThinMaterial, in: Capsule())
                .padding()
        case .failed(let message):
            VStack(alignment: .leading, spacing: 12) {
                Text("Could not play \(item.title)").font(.headline)
                Text(message)
                Button("Close") { dismiss() }
            }
            .padding()
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
            .padding()
        case .playing, nil:
            #if os(iOS)
            Button("Close", systemImage: "xmark") { dismiss() }
                .labelStyle(.iconOnly)
                .padding()
            #endif
        }
    }

    private func resolve() async {
        guard let core = model.core else { return }
        let item = item
        do {
            resolved = try await Task.detached { () throws -> Resolved in
                switch item {
                case .channel(let id, _): .live(try core.tuneLive(channelId: id))
                case .movie(let id, _, let startAt): .vod(try core.tuneMovie(movieId: id, startAtSecs: startAt))
                case .episode(let id, _, let startAt): .vod(try core.tuneEpisode(episodeId: id, startAtSecs: startAt))
                case .catchup(let id, _, let start, let stop):
                    .replay(try core.tuneCatchup(channelId: id, start: start, stop: stop))
                }
            }.value
        } catch {
            status = .failed(error.localizedDescription)
        }
    }

    private func playIfReady() {
        guard !started, let resolved, let controller, let core = model.core else { return }
        started = true
        switch resolved {
        case .live(let tune): controller.play(tune, core: core)
        case .vod(let tune): controller.play(tune, core: core, profileId: model.profileId)
        case .replay(let tune): controller.play(tune, core: core)
        }
    }
}

/// Hosts the UIKit player controller in SwiftUI.
struct MPVPlayerView: UIViewControllerRepresentable {
    @Binding var controller: MPVPlayerController?
    var onStatus: (MPVPlayerController.PlayerStatus) -> Void

    func makeUIViewController(context: Context) -> MPVPlayerController {
        let player = MPVPlayerController()
        player.onStatus = onStatus
        DispatchQueue.main.async { controller = player }
        return player
    }

    func updateUIViewController(_ player: MPVPlayerController, context: Context) {
        player.onStatus = onStatus
    }
}
