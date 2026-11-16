import Foundation
import Libmpv
import UIKit

/// libmpv, drawing into a Metal layer: the Apple counterpart of
/// `aurora-player/src/mpv.rs` (docs/APPLE_PORT_AND_SYNC.md §2.3).
///
/// What mpv is told about a stream is not decided here. Every tune's `mpvOptions` comes
/// from `aurora_player::backend::LoadOptions::mpv_options`, the function that configures
/// mpv on Windows, and is applied the same way: as properties, before `loadfile`. Only the
/// video output is Apple-specific — Vulkan through MoltenVK onto a `CAMetalLayer`, and
/// VideoToolbox for decoding — following MPVKit's own demo.
///
/// Live failover is the spike's version of `aurora-app/src/playback.rs`: try each source
/// in order, and report every outcome to the core so the next tune starts from the one
/// that worked. Films and episodes save their position through the core, which decides
/// what counts as finished (`aurora_db::repo::progress::save`).
final class MPVPlayerController: UIViewController {
    private let metalLayer = MetalLayer()
    private var mpv: OpaquePointer?
    /// mpv's events are drained here, and all session state below is touched only here.
    private let queue = DispatchQueue(label: "tv.aurora.mpv", qos: .userInitiated)

    private enum Session {
        case live(LiveTune, sourceIndex: Int)
        case vod(VodTune, profileId: Int64, lastSaved: Date)
    }

    private var core: AuroraCore?
    private var session: Session?

    /// How often a playing film's position is written. Often enough that a crash loses
    /// little, rarely enough that the library is not written every second.
    private let progressInterval: TimeInterval = 15

    /// Told what is happening, on the main thread.
    var onStatus: ((PlayerStatus) -> Void)?

    enum PlayerStatus: Equatable {
        case opening(source: Int, of: Int)
        case playing
        case failed(String)
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        metalLayer.frame = view.bounds
        metalLayer.contentsScale = view.traitCollection.displayScale
        metalLayer.framebufferOnly = true
        metalLayer.backgroundColor = UIColor.black.cgColor
        view.layer.addSublayer(metalLayer)
        createMpv()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        metalLayer.frame = view.bounds
    }

    deinit {
        guard let handle = mpv else { return }
        // The wakeup callback holds this controller unretained. Detach it first, then let
        // any drain already queued finish, so nothing touches `self` after it is gone.
        mpv_set_wakeup_callback(handle, nil, nil)
        queue.sync {}
        mpv_terminate_destroy(handle)
    }

    // MARK: Playing

    /// Start a live channel: apply its options, then try its sources in order.
    func play(_ tune: LiveTune, core: AuroraCore) {
        queue.async { [self] in
            self.core = core
            session = .live(tune, sourceIndex: 0)
            apply(tune.mpvOptions)
            loadLiveSource()
        }
    }

    /// Start a film or episode. Its start position is already in its options.
    func play(_ tune: VodTune, core: AuroraCore, profileId: Int64) {
        queue.async { [self] in
            self.core = core
            session = .vod(tune, profileId: profileId, lastSaved: .distantPast)
            apply(tune.mpvOptions)
            DispatchQueue.main.async { self.onStatus?(.opening(source: 1, of: 1)) }
            // Can carry the provider's password (R-126): handed to mpv, never logged.
            command("loadfile", tune.url, "replace")
        }
    }

    func stop() {
        queue.async { [self] in
            saveProgress(force: true)
            session = nil
            command("stop")
        }
    }

    func togglePause() {
        queue.async { [self] in
            guard let mpv else { return }
            var paused: Int32 = 0
            mpv_get_property(mpv, "pause", MPV_FORMAT_FLAG, &paused)
            var next: Int32 = paused == 0 ? 1 : 0
            mpv_set_property(mpv, "pause", MPV_FORMAT_FLAG, &next)
            if next == 1 { saveProgress(force: true) }
        }
    }

    /// Jump by `seconds`, forwards or back. Films and episodes only: live has no
    /// timeshift on Apple yet.
    func seek(by seconds: Double) {
        queue.async { [self] in
            guard case .vod = session else { return }
            command("seek", String(seconds), "relative")
        }
    }

    // MARK: Session steps (on `queue`)

    private func apply(_ options: [MpvOption]) {
        guard let mpv else { return }
        for option in options {
            let status = mpv_set_property_string(mpv, option.name, option.value)
            if status < 0 {
                // The same note Windows logs ("mpv rejected {key}"). Never the value: a
                // header can carry a token.
                NSLog("mpv rejected %@: %@", option.name, String(cString: mpv_error_string(status)))
            }
        }
    }

    private func loadLiveSource() {
        guard case .live(let tune, let index)? = session, index < tune.sources.count else { return }
        let total = tune.sources.count
        DispatchQueue.main.async { self.onStatus?(.opening(source: index + 1, of: total)) }
        command("loadfile", tune.sources[index].url, "replace")
    }

    private func fileLoaded() {
        switch session {
        case .live(let tune, let index)?:
            if index < tune.sources.count { report(tune.sources[index].sourceId, opened: true) }
        case .vod?, nil:
            break
        }
        DispatchQueue.main.async { self.onStatus?(.playing) }
    }

    private func fileFailed(_ reason: String) {
        switch session {
        case .live(let tune, let index)?:
            report(tune.sources[index].sourceId, opened: false)
            if index + 1 < tune.sources.count {
                session = .live(tune, sourceIndex: index + 1)
                loadLiveSource()
                return
            }
            session = nil
            let message = tune.sources.count == 1
                ? "\(tune.title) would not play: \(reason)."
                : "None of the \(tune.sources.count) sources for \(tune.title) would play. The last said: \(reason)."
            DispatchQueue.main.async { self.onStatus?(.failed(message)) }
        case .vod(let tune, _, _)?:
            session = nil
            DispatchQueue.main.async { self.onStatus?(.failed("\(tune.title) would not play: \(reason).")) }
        case nil:
            break
        }
    }

    /// Write a film's position if it is due, or now when `force` is set.
    private func saveProgress(force: Bool) {
        guard case .vod(let tune, let profileId, let lastSaved)? = session, let mpv, let core else { return }
        guard force || Date().timeIntervalSince(lastSaved) >= progressInterval else { return }
        var position = 0.0
        var duration = 0.0
        guard mpv_get_property(mpv, "time-pos", MPV_FORMAT_DOUBLE, &position) >= 0,
              mpv_get_property(mpv, "duration", MPV_FORMAT_DOUBLE, &duration) >= 0,
              duration > 0
        else { return }
        session = .vod(tune, profileId: profileId, lastSaved: Date())
        let kind: VodKind = tune.kind
        do {
            _ = try core.saveProgress(
                profileId: profileId, kind: kind, id: tune.id,
                positionSecs: Int64(position), durationSecs: Int64(duration)
            )
        } catch {
            NSLog("could not save the position: %@", error.localizedDescription)
        }
    }

    private func report(_ sourceId: Int64, opened: Bool) {
        do {
            try core?.reportSource(sourceId: sourceId, opened: opened)
        } catch {
            // Ranking is a hint for the next tune; failing to record it must not stop
            // this one.
            NSLog("could not record the source outcome: %@", error.localizedDescription)
        }
    }

    // MARK: mpv

    private func createMpv() {
        guard let handle = mpv_create() else {
            onStatus?(.failed("The video player could not start."))
            return
        }
        mpv = handle
        var layer = metalLayer
        mpv_set_option(handle, "wid", MPV_FORMAT_INT64, &layer)
        mpv_set_option_string(handle, "vo", "gpu-next")
        mpv_set_option_string(handle, "gpu-api", "vulkan")
        mpv_set_option_string(handle, "gpu-context", "moltenvk")
        mpv_set_option_string(handle, "hwdec", "videotoolbox")
        mpv_set_option_string(handle, "video-rotate", "no")
        #if DEBUG
        mpv_request_log_messages(handle, "warn")
        #endif
        guard mpv_initialize(handle) >= 0 else {
            mpv_terminate_destroy(handle)
            mpv = nil
            onStatus?(.failed("The video player could not start."))
            return
        }
        // Position changes drive the periodic progress save.
        mpv_observe_property(handle, 0, "time-pos", MPV_FORMAT_DOUBLE)
        mpv_set_wakeup_callback(handle, { context in
            guard let context else { return }
            let controller = Unmanaged<MPVPlayerController>.fromOpaque(context).takeUnretainedValue()
            controller.queue.async { controller.drainEvents() }
        }, Unmanaged.passUnretained(self).toOpaque())
    }

    /// On `queue`.
    private func drainEvents() {
        while let mpv, let event = mpv_wait_event(mpv, 0)?.pointee, event.event_id != MPV_EVENT_NONE {
            switch event.event_id {
            case MPV_EVENT_FILE_LOADED:
                fileLoaded()
            case MPV_EVENT_END_FILE:
                guard let data = event.data else { break }
                let end = data.assumingMemoryBound(to: mpv_event_end_file.self).pointee
                if end.reason == MPV_END_FILE_REASON_ERROR {
                    fileFailed(String(cString: mpv_error_string(end.error)))
                } else if end.reason == MPV_END_FILE_REASON_EOF {
                    // Played to the end: the last position is the duration.
                    saveProgress(force: true)
                }
                // `replace` and `stop` also end the previous file; neither is a failure.
            case MPV_EVENT_PROPERTY_CHANGE:
                saveProgress(force: false)
            case MPV_EVENT_LOG_MESSAGE:
                #if DEBUG
                if let data = event.data {
                    let message = data.assumingMemoryBound(to: mpv_event_log_message.self).pointee
                    NSLog("mpv [%@] %@", String(cString: message.prefix), String(cString: message.text))
                }
                #endif
            case MPV_EVENT_SHUTDOWN:
                return
            default:
                break
            }
        }
    }

    /// On `queue`.
    private func command(_ name: String, _ args: String...) {
        guard let mpv else { return }
        let strings = [name] + args
        var cArgs: [UnsafePointer<CChar>?] = strings.map { UnsafePointer(strdup($0)) }
        cArgs.append(nil)
        defer { cArgs.forEach { free(UnsafeMutableRawPointer(mutating: $0)) } }
        let status = mpv_command(mpv, &cArgs)
        if status < 0 {
            NSLog("mpv %@ failed: %@", name, String(cString: mpv_error_string(status)))
        }
    }
}

/// A `CAMetalLayer` that ignores MoltenVK's 1x1 drawable size, which otherwise causes
/// flicker and can leave the picture stuck at one pixel (mpv-player/mpv#13651). From
/// MPVKit's demo.
final class MetalLayer: CAMetalLayer {
    override var drawableSize: CGSize {
        get { super.drawableSize }
        set {
            if Int(newValue.width) > 1 && Int(newValue.height) > 1 {
                super.drawableSize = newValue
            }
        }
    }
}
