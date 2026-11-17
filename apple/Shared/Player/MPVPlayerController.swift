import Foundation
import Libmpv
import UIKit

/// libmpv, drawing into a Metal layer: the Apple counterpart of
/// `aurora-video/src/libmpv.rs` (docs/APPLE_PORT_AND_SYNC.md §2.3).
///
/// What mpv is told about a stream is not decided here. Every tune's `mpvOptions` comes
/// from `aurora_video::Request::settings`, the function that configures mpv on Windows,
/// and is applied the same way: as properties, before `loadfile`. Only the
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
        /// A programme that already aired, from the provider's archive. Seekable like a
        /// film, but no watch progress: Windows doesn't keep any for catch-up either.
        case replay(CatchupTune)
    }

    private var core: AuroraCore?
    private var session: Session?

    /// How often a playing film's position is written. Often enough that a crash loses
    /// little, rarely enough that the library is not written every second.
    private let progressInterval: TimeInterval = 15

    /// Told what is happening, on the main thread.
    var onStatus: ((PlayerStatus) -> Void)?
    /// Where playback is and what the file offers, on the main thread: at most four
    /// times a second while it plays, at once when anything else changes.
    var onState: ((PlaybackState) -> Void)?
    /// The file played to its end, on the main thread.
    var onEnded: (() -> Void)?

    /// What the controls show. Read from mpv on `queue`, handed over whole.
    struct PlaybackState: Equatable {
        var position = 0.0
        /// 0 when the stream doesn't say, which a provider's stream often doesn't.
        var duration = 0.0
        var paused = false
        var speed = 1.0
        var audioTracks: [Track] = []
        var subtitleTracks: [Track] = []
        var audioId: Int64?
        var subtitleId: Int64?
        /// The file is open: before this mpv has no chapters or tracks to tell.
        var loaded = false
    }

    /// An audio or subtitle track, as Windows' menu lists it (`model.ts` `Track`).
    struct Track: Identifiable, Equatable {
        let id: Int64
        let title: String?
        let language: String?
        let codec: String?
        let channels: String?

        /// `trackName` in `model.ts`.
        var name: String { title ?? language ?? "Track \(id)" }
        var detail: String { [language, codec, channels].compactMap { $0 }.joined(separator: " · ") }
    }

    private var loaded = false
    private var tracks: (audio: [Track], subtitles: [Track], audioId: Int64?, subtitleId: Int64?) = ([], [], nil, nil)
    private var lastPublished = Date.distantPast

    /// What each observed property is, in mpv's `reply_userdata`.
    private enum Observed: UInt64 {
        case position, duration, pause, speed, tracks
    }

    enum PlayerStatus: Equatable {
        case opening(source: Int, of: Int)
        case playing
        /// Sound, but the picture couldn't start: the video output (Vulkan through
        /// MoltenVK) failed on this device. Playback carries on, so say so rather than
        /// leave a black screen with audio and no explanation.
        case noPicture(String)
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
            resetFile()
            apply(tune.mpvOptions)
            loadLiveSource()
        }
    }

    /// Start a film or episode. Its start position is already in its options.
    func play(_ tune: VodTune, core: AuroraCore, profileId: Int64) {
        queue.async { [self] in
            self.core = core
            session = .vod(tune, profileId: profileId, lastSaved: .distantPast)
            resetFile()
            apply(tune.mpvOptions)
            DispatchQueue.main.async { self.onStatus?(.opening(source: 1, of: 1)) }
            // Can carry the provider's password (R-126): handed to mpv, never logged.
            command("loadfile", tune.url, "replace")
        }
    }

    /// Replay a programme that already aired.
    func play(_ tune: CatchupTune, core: AuroraCore) {
        queue.async { [self] in
            self.core = core
            session = .replay(tune)
            resetFile()
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

    func setPaused(_ paused: Bool) {
        queue.async { [self] in
            guard let mpv else { return }
            var flag: Int32 = paused ? 1 : 0
            mpv_set_property(mpv, "pause", MPV_FORMAT_FLAG, &flag)
            if paused { saveProgress(force: true) }
        }
    }

    /// Playback speed, kept to what the menu offers (`clamp_speed`).
    func setSpeed(_ speed: Double) {
        queue.async { [self] in
            guard let mpv else { return }
            var value = clampSpeed(speed: speed)
            mpv_set_property(mpv, "speed", MPV_FORMAT_DOUBLE, &value)
        }
    }

    func setAudioTrack(_ id: Int64) {
        queue.async { [self] in
            guard let mpv else { return }
            mpv_set_property_string(mpv, "aid", String(id))
        }
    }

    /// `nil` turns subtitles off.
    func setSubtitleTrack(_ id: Int64?) {
        queue.async { [self] in
            guard let mpv else { return }
            mpv_set_property_string(mpv, "sid", id.map(String.init) ?? "no")
        }
    }

    /// The picture's shape, with the properties Windows sets for it
    /// (`aurora_video::Aspect::mpv`, through `aspect_options`).
    func setAspect(_ aspect: PictureAspect) {
        queue.async { [self] in apply(aspectOptions(aspect: aspect)) }
    }

    /// Jump to `seconds` from the start. Films, episodes and catch-up only.
    func seek(to seconds: Double) {
        queue.async { [self] in
            switch session {
            case .vod?, .replay?: break
            case .live?, nil: return
            }
            command("seek", String(max(0, seconds)), "absolute")
        }
    }

    /// The open file's chapters, for `sync_chapters`. Empty before the file is open.
    func chapters(_ done: @escaping ([ChapterMark]) -> Void) {
        queue.async { [self] in
            var marks: [ChapterMark] = []
            if let mpv {
                var count: Int64 = 0
                if mpv_get_property(mpv, "chapter-list/count", MPV_FORMAT_INT64, &count) >= 0 {
                    for i in 0..<count {
                        var time = 0.0
                        guard mpv_get_property(mpv, "chapter-list/\(i)/time", MPV_FORMAT_DOUBLE, &time) >= 0
                        else { continue }
                        marks.append(ChapterMark(title: string("chapter-list/\(i)/title"), startSecs: time))
                    }
                }
            }
            DispatchQueue.main.async { done(marks) }
        }
    }

    /// Jump by `seconds`, forwards or back. Films, episodes and catch-up: live has no
    /// timeshift on Apple yet.
    func seek(by seconds: Double) {
        queue.async { [self] in
            switch session {
            case .vod?, .replay?: break
            case .live?, nil: return
            }
            command("seek", String(seconds), "relative")
        }
    }

    // MARK: Session steps (on `queue`)

    /// A tune's settings, applied as Windows' engine applies them (`aurora-video`
    /// `Engine::load`): every one, every file, so nothing from the last file lingers.
    private func apply(_ options: [MpvOption]) {
        guard let mpv else { return }
        for option in options {
            let name: String
            var status: Int32
            switch option {
            case .text(let key, let value):
                name = key
                status = mpv_set_property_string(mpv, key, value)
                // `start=none` is how a file is told to start at the beginning; an mpv
                // that doesn't take the word takes a zero (as on Windows).
                if status < 0, key == "start" {
                    status = mpv_set_property_string(mpv, "start", "0")
                }
            case .list(let key, let items):
                name = key
                // Cleared, then item by item: an empty list takes the last file's away,
                // and an item holding a comma (a header value) is not split in two.
                status = run(["change-list", key, "clr", ""])
                for item in items where status >= 0 {
                    status = run(["change-list", key, "append", item])
                }
            }
            if status < 0 {
                // The same note Windows logs ("mpv refused a setting"). Never the value: a
                // header can carry a token.
                NSLog("mpv rejected %@: %@", name, String(cString: mpv_error_string(status)))
            }
        }
    }

    private func loadLiveSource() {
        guard case .live(let tune, let index)? = session, index < tune.sources.count else { return }
        let total = tune.sources.count
        DispatchQueue.main.async { self.onStatus?(.opening(source: index + 1, of: total)) }
        command("loadfile", tune.sources[index].url, "replace")
    }

    /// A new file is starting: nothing of the last one's state carries over.
    private func resetFile() {
        loaded = false
        tracks = ([], [], nil, nil)
        publish(force: true)
    }

    private func fileLoaded() {
        loaded = true
        readTracks()
        publish(force: true)
        switch session {
        case .live(let tune, let index)?:
            if index < tune.sources.count { report(tune.sources[index].sourceId, opened: true) }
        case .vod?, .replay?, nil:
            break
        }
        let status: PlayerStatus = hasVideoTrack() && currentVideoOutput() == nil
            ? .noPicture("The picture couldn't start on this device's graphics, so only the sound is playing.")
            : .playing
        DispatchQueue.main.async { self.onStatus?(status) }
    }

    /// Hand the controls where playback is. On `queue`.
    private func publish(force: Bool) {
        guard force || Date().timeIntervalSince(lastPublished) >= 0.25 else { return }
        lastPublished = Date()
        var state = PlaybackState()
        state.loaded = loaded
        if loaded, let mpv {
            mpv_get_property(mpv, "time-pos", MPV_FORMAT_DOUBLE, &state.position)
            mpv_get_property(mpv, "duration", MPV_FORMAT_DOUBLE, &state.duration)
            var paused: Int32 = 0
            mpv_get_property(mpv, "pause", MPV_FORMAT_FLAG, &paused)
            state.paused = paused != 0
            mpv_get_property(mpv, "speed", MPV_FORMAT_DOUBLE, &state.speed)
        }
        if !state.position.isFinite { state.position = 0 }
        if !state.duration.isFinite { state.duration = 0 }
        state.audioTracks = tracks.audio
        state.subtitleTracks = tracks.subtitles
        state.audioId = tracks.audioId
        state.subtitleId = tracks.subtitleId
        DispatchQueue.main.async { self.onState?(state) }
    }

    /// The file's audio and subtitle tracks, and which of each is on. On `queue`.
    private func readTracks() {
        guard let mpv else { return }
        var count: Int64 = 0
        guard mpv_get_property(mpv, "track-list/count", MPV_FORMAT_INT64, &count) >= 0 else { return }
        var audio: [Track] = [], subtitles: [Track] = []
        var audioId: Int64?, subtitleId: Int64?
        for i in 0..<count {
            let base = "track-list/\(i)"
            var id: Int64 = 0
            guard mpv_get_property(mpv, "\(base)/id", MPV_FORMAT_INT64, &id) >= 0 else { continue }
            var selected: Int32 = 0
            mpv_get_property(mpv, "\(base)/selected", MPV_FORMAT_FLAG, &selected)
            var channels: String?
            var channelCount: Int64 = 0
            if mpv_get_property(mpv, "\(base)/demux-channel-count", MPV_FORMAT_INT64, &channelCount) >= 0,
               channelCount > 0 {
                channels = string("\(base)/demux-channels") ?? "\(channelCount) ch"
            }
            let track = Track(
                id: id,
                title: string("\(base)/title"),
                language: string("\(base)/lang"),
                codec: string("\(base)/codec"),
                channels: channels
            )
            switch string("\(base)/type") {
            case "audio":
                audio.append(track)
                if selected != 0 { audioId = id }
            case "sub":
                subtitles.append(Track(id: id, title: track.title, language: track.language,
                                       codec: track.codec, channels: nil))
                if selected != 0 { subtitleId = id }
            default:
                break
            }
        }
        tracks = (audio, subtitles, audioId, subtitleId)
    }

    /// A string property, or nil when mpv has none (or an empty one). On `queue`.
    private func string(_ name: String) -> String? {
        guard let mpv, let raw = mpv_get_property_string(mpv, name) else { return nil }
        defer { mpv_free(raw) }
        let value = String(cString: raw)
        return value.isEmpty ? nil : value
    }

    /// Whether the file has a picture to show at all: a radio stream has none, and
    /// that's not a failure.
    private func hasVideoTrack() -> Bool {
        guard let mpv else { return false }
        var count: Int64 = 0
        guard mpv_get_property(mpv, "track-list/count", MPV_FORMAT_INT64, &count) >= 0 else { return false }
        return (0..<count).contains { i in
            guard let raw = mpv_get_property_string(mpv, "track-list/\(i)/type") else { return false }
            defer { mpv_free(raw) }
            return String(cString: raw) == "video"
        }
    }

    /// The video output mpv is drawing with, or nil when none could start.
    private func currentVideoOutput() -> String? {
        guard let mpv, let raw = mpv_get_property_string(mpv, "current-vo") else { return nil }
        defer { mpv_free(raw) }
        let name = String(cString: raw)
        return name.isEmpty ? nil : name
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
        case .replay(let tune)?:
            session = nil
            DispatchQueue.main.async {
                self.onStatus?(.failed("\(tune.title)'s archive would not play: \(reason)."))
            }
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
            NSLog("could not save the position: %@", error.viewerMessage)
        }
    }

    private func report(_ sourceId: Int64, opened: Bool) {
        do {
            try core?.reportSource(sourceId: sourceId, opened: opened)
        } catch {
            // Ranking is a hint for the next tune; failing to record it must not stop
            // this one.
            NSLog("could not record the source outcome: %@", error.viewerMessage)
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
        #if targetEnvironment(simulator) && arch(x86_64)
        // The simulator on an Intel Mac has no GPU MoltenVK can draw with, and as of
        // 2026-11-17 its Metal driver crashes the app (EXC_BAD_ACCESS in MTLSimDriver's
        // -newIndirectArgumentBufferLayoutWithStructType:) while MoltenVK probes it. Don't
        // start the picture there: the sound still plays, and since no video output
        // starts, the player shows its "only the sound is playing" notice.
        mpv_set_option_string(handle, "vid", "no")
        #endif
        #if DEBUG
        // `-AuroraMpvLog v` (or `debug`) at launch for mpv's full story; warnings otherwise.
        mpv_request_log_messages(handle, UserDefaults.standard.string(forKey: "AuroraMpvLog") ?? "warn")
        #endif
        guard mpv_initialize(handle) >= 0 else {
            mpv_terminate_destroy(handle)
            mpv = nil
            onStatus?(.failed("The video player could not start."))
            return
        }
        // Position changes drive the periodic progress save and the controls; the rest
        // only the controls. The track list's selections change with `aid` and `sid`.
        mpv_observe_property(handle, Observed.position.rawValue, "time-pos", MPV_FORMAT_DOUBLE)
        mpv_observe_property(handle, Observed.duration.rawValue, "duration", MPV_FORMAT_DOUBLE)
        mpv_observe_property(handle, Observed.pause.rawValue, "pause", MPV_FORMAT_FLAG)
        mpv_observe_property(handle, Observed.speed.rawValue, "speed", MPV_FORMAT_DOUBLE)
        mpv_observe_property(handle, Observed.tracks.rawValue, "track-list", MPV_FORMAT_NONE)
        mpv_observe_property(handle, Observed.tracks.rawValue, "aid", MPV_FORMAT_NONE)
        mpv_observe_property(handle, Observed.tracks.rawValue, "sid", MPV_FORMAT_NONE)
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
                    DispatchQueue.main.async { self.onEnded?() }
                }
                // `replace` and `stop` also end the previous file; neither is a failure.
            case MPV_EVENT_PROPERTY_CHANGE:
                switch Observed(rawValue: event.reply_userdata) {
                case .position?:
                    saveProgress(force: false)
                    publish(force: false)
                case .tracks?:
                    if loaded { readTracks() }
                    publish(force: true)
                case .duration?, .pause?, .speed?, nil:
                    publish(force: true)
                }
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
        let status = run([name] + args)
        if status < 0 {
            NSLog("mpv %@ failed: %@", name, String(cString: mpv_error_string(status)))
        }
    }

    /// An mpv command and its status. Its arguments are never logged: a URL or a header
    /// can carry the provider's password.
    private func run(_ strings: [String]) -> Int32 {
        guard let mpv else { return MPV_ERROR_UNINITIALIZED.rawValue }
        var cArgs: [UnsafePointer<CChar>?] = strings.map { UnsafePointer(strdup($0)) }
        cArgs.append(nil)
        defer { cArgs.forEach { free(UnsafeMutableRawPointer(mutating: $0)) } }
        return mpv_command(mpv, &cArgs)
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
