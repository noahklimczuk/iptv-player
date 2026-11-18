import Foundation
import Observation

/// What the controls know and do, for one visit to the player: the Apple counterpart of
/// `src-ui` `PlayerOverlay`, `useChrome`, `useEpisodeAids` and `useSleepTimer`.
///
/// The rules are the core's (`player.rs`, `sleep.rs`, mirrored from Windows); this only
/// keeps time and passes things along: when to ask for the markers again, when the
/// playhead is inside one, when the viewer jumped by hand, when Up Next is due, and when
/// the sleep timer has run out.
@MainActor @Observable
final class PlayerSession {
    enum Panel: Hashable {
        case subtitles, audio, picture
    }

    let menus = playerMenus()

    private(set) var item: PlayerItem
    private(set) var state = MPVPlayerController.PlaybackState()
    var status: MPVPlayerController.PlayerStatus?

    // MARK: Controls

    private(set) var controlsVisible = true
    var panel: Panel? {
        didSet { poke() }
    }
    private(set) var aspect: PictureAspect = .auto
    private var lastActivity = Date()

    // MARK: Episode aids (`useEpisodeAids`)

    private(set) var aids: PlaybackAids?
    /// The stage the aids were last asked for at; a new one asks again.
    private var aidsStage: String?
    /// Bumped when a hand seek taught the library something, to ask again.
    private var evidence = 0
    private var chaptersSynced: Int64?
    /// Markers already jumped over by themselves this episode, so seeking back into an
    /// intro doesn't throw the viewer forward again.
    private var autoSkipped: Set<String> = []
    /// Where our own skip will land, so it isn't read back as the viewer's jump.
    private var ourSeek: Double?
    private var lastPosition: Double?
    private var upNextDismissed: Int64?
    private(set) var upNextDeadline: Date?

    // MARK: Sleep timer

    private(set) var sleep = SleepState(atUnix: nil, after: nil, remainingSecs: nil, warning: false)
    /// The timer ran out: the screen closes.
    private(set) var sleptAt: Date?

    weak var controller: MPVPlayerController?
    let core: AuroraCore
    private let profileId: Int64
    private var ticker: Timer?

    init(item: PlayerItem, core: AuroraCore, profileId: Int64) {
        self.item = item
        self.core = core
        self.profileId = profileId
        sleep = core.sleepState()
    }

    var isLive: Bool {
        if case .channel = item { return true }
        return false
    }

    var seekable: Bool { !isLive }

    var episodeId: Int64? {
        if case .episode(let id, _, _) = item { return id }
        return nil
    }

    // MARK: Ticking

    func start() {
        ticker?.invalidate()
        ticker = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    func stop() {
        ticker?.invalidate()
        ticker = nil
    }

    private func tick() {
        // Controls fade after a while untouched (`useChrome`), unless paused or a menu is
        // open: then the viewer is looking at them.
        if controlsVisible, panel == nil, !state.paused, state.loaded, !holdsControls,
           Date().timeIntervalSince(lastActivity) * 1000 >= Double(menus.hideControlsAfterMs) {
            controlsVisible = false
        }
        // The remote's jump hint goes after its moment, even when nothing else redraws.
        if let until = seekHintUntil, Date() >= until { seekHintUntil = nil }
        sleep = core.sleepState()
        if core.sleepDue() {
            controller?.stop()
            sleptAt = Date()
        }
        if let deadline = upNextDeadline, Date() >= deadline {
            upNextDeadline = nil
            playNext()
        }
    }

    /// Opening or failed: the controls stay, there is nothing else to look at.
    private var holdsControls: Bool {
        switch status {
        case .opening?, .failed?: true
        case .playing?, .noPicture?, nil: false
        }
    }

    /// The viewer did something: show the controls and start the fade over.
    func poke() {
        lastActivity = Date()
        controlsVisible = true
    }

    func hideControls() {
        controlsVisible = false
    }

    func toggleControls() {
        if controlsVisible { hideControls() } else { poke() }
    }

    // MARK: Playback

    func update(_ new: MPVPlayerController.PlaybackState) {
        state = new
        guard let episodeId else { return }
        watchForHandSeek(episodeId)
        refreshAids(episodeId)
        autoSkipIfAsked()
        updateUpNext()
    }

    func togglePause() {
        controller?.setPaused(!state.paused)
        poke()
    }

    func seek(by seconds: Double) {
        guard seekable else { return }
        controller?.seek(by: seconds)
        poke()
    }

    /// A jump from the remote with the controls hidden: it shows where it landed, not
    /// the whole set of controls, so the next press jumps again.
    func nudge(by seconds: Double) {
        guard seekable else { return }
        controller?.seek(by: seconds)
        seekHintUntil = Date().addingTimeInterval(2)
    }

    /// Until when the remote's last jump shows the progress bar alone.
    private(set) var seekHintUntil: Date?

    func seek(to seconds: Double) {
        guard seekable else { return }
        controller?.seek(to: seconds)
        poke()
    }

    func setSpeed(_ speed: Double) {
        controller?.setSpeed(speed)
        poke()
    }

    func setAspect(_ aspect: PictureAspect) {
        self.aspect = aspect
        controller?.setAspect(aspect)
        poke()
    }

    func setAudioTrack(_ id: Int64) {
        controller?.setAudioTrack(id)
        poke()
    }

    func setSubtitleTrack(_ id: Int64?) {
        controller?.setSubtitleTrack(id)
        poke()
    }

    /// The film or episode played out. Up Next plays if it was going to; otherwise the
    /// screen closes, as there is nothing left to watch.
    func ended() -> Bool {
        if let aids, aids.nextEpisode != nil, aids.prefs.autoplayNext, upNextDismissed != episodeId {
            playNext()
            return true
        }
        return false
    }

    // MARK: Episode aids

    /// Ask for the markers again whenever more is known: the file opened (its chapters
    /// can be read), its length arrived, or a hand seek taught the library something.
    private func refreshAids(_ episodeId: Int64) {
        let stage = "\(episodeId)-\(state.duration > 0)-\(state.loaded)-\(evidence)"
        guard stage != aidsStage else { return }
        aidsStage = stage
        let duration = state.duration
        let core = core, profileId = profileId
        let fetch = { [weak self] (chapters: [ChapterMark]?) -> Void in
            _ = Task.detached {
                if let chapters {
                    // Not fatal: the worst case is the file's own chapters aren't used.
                    _ = try? core.syncChapters(episodeId: episodeId, chapters: chapters, durationSecs: duration)
                }
                let aids = try? core.playbackAids(profileId: profileId, episodeId: episodeId, durationSecs: duration)
                await MainActor.run {
                    guard let self, self.episodeId == episodeId, let aids else { return }
                    self.aids = aids
                    self.updateUpNext()
                }
            }
        }
        // Chapters once per episode, once the file is open and its length known.
        if state.loaded, duration > 0, chaptersSynced != episodeId, let controller {
            chaptersSynced = episodeId
            controller.chapters { fetch($0) }
        } else {
            fetch(nil)
        }
    }

    /// The marker the playhead is inside, if any.
    var activeMarker: SkipMarker? {
        guard let aids, episodeId != nil else { return nil }
        let pos = state.position
        return aids.markers.first { pos >= $0.startSecs && pos < $0.endSecs - 0.5 }
    }

    private func autoSkipIfAsked() {
        guard let marker = activeMarker, let aids, marker.mayAutoSkip else { return }
        let asked = (marker.kind == .intro && aids.prefs.alwaysSkipIntro)
            || (marker.kind == .recap && aids.prefs.alwaysSkipRecap)
        guard asked else { return }
        let key = "\(marker.kind)-\(marker.startSecs)"
        guard !autoSkipped.contains(key) else { return }
        autoSkipped.insert(key)
        skip(marker, automatic: true)
    }

    /// Jump past a marker. A press teaches the library where this show's intro is; an
    /// automatic jump only replays what it already knew.
    func skip(_ marker: SkipMarker, automatic: Bool = false) {
        ourSeek = marker.endSecs
        controller?.seek(to: marker.endSecs)
        guard !automatic, let episodeId else { return }
        let core = core, duration = state.duration
        Task.detached {
            do {
                try core.recordSkip(episodeId: episodeId, kind: marker.kind, startSecs: marker.startSecs,
                                    endSecs: marker.endSecs, durationSecs: duration)
            } catch {
                NSLog("could not remember that skip: %@", error.viewerMessage)
            }
        }
    }

    /// A forward jump the viewer made themselves, offered as intro evidence. The first
    /// sample after the file opens is never compared, so resuming isn't a jump from 0.
    private func watchForHandSeek(_ episodeId: Int64) {
        guard state.loaded else {
            lastPosition = nil
            return
        }
        let previous = lastPosition
        lastPosition = state.position
        guard let previous, state.position - previous >= 5 else { return }
        if let aimed = ourSeek, abs(state.position - aimed) < 2 {
            ourSeek = nil
            return
        }
        let core = core, from = previous, to = state.position, duration = state.duration
        Task.detached {
            // Not fatal: the worst case is the library learns nothing from this jump.
            let taken = (try? core.observeSeek(episodeId: episodeId, fromSecs: from, toSecs: to,
                                               durationSecs: duration)) ?? false
            if taken {
                await MainActor.run { [weak self] in
                    if self?.episodeId == episodeId { self?.evidence += 1 }
                }
            }
        }
    }

    // MARK: Up Next

    var upNextVisible: Bool {
        guard let aids, aids.nextEpisode != nil, let at = aids.upNextAtSecs else { return false }
        return state.position >= at && upNextDismissed != episodeId
    }

    var upNextLeft: Int {
        guard let upNextDeadline else { return Int(menus.upNextCountdownSecs) }
        return max(0, Int(upNextDeadline.timeIntervalSinceNow.rounded(.up)))
    }

    /// The countdown runs against a deadline, only while the card is up and only when the
    /// show plays the next episode by itself.
    private func updateUpNext() {
        if upNextVisible, aids?.prefs.autoplayNext == true {
            if upNextDeadline == nil {
                upNextDeadline = Date().addingTimeInterval(Double(menus.upNextCountdownSecs))
            }
        } else {
            upNextDeadline = nil
        }
    }

    func dismissUpNext() {
        upNextDismissed = episodeId
        upNextDeadline = nil
    }

    /// Start the next episode in place.
    func playNext() {
        guard let next = aids?.nextEpisode else { return }
        upNextDeadline = nil
        let title = next.title.map { "S\(next.season)E\(next.episode) · \($0)" } ?? "S\(next.season)E\(next.episode)"
        item = .episode(id: next.id, title: title, startAt: nil)
        aids = nil
        aidsStage = nil
        autoSkipped = []
        ourSeek = nil
        lastPosition = nil
        state = MPVPlayerController.PlaybackState()
        status = nil
        poke()
    }

    // MARK: Sleep timer

    func setSleep(_ after: SleepAfter) throws {
        var channelId: Int64?
        if case .channel(let id, _) = item { channelId = id }
        let remaining = state.duration > 0 ? max(0, state.duration - state.position) : nil
        sleep = try core.sleepSet(after: after, playing: SleepPlaying(
            channelId: channelId, remainingSecs: remaining, speed: state.speed
        ))
        poke()
    }

    func extendSleep() {
        sleep = (try? core.sleepExtend(minutes: menus.sleepExtendMinutes)) ?? core.sleepState()
        poke()
    }

    func cancelSleep() {
        sleep = core.sleepCancel()
        poke()
    }
}

/// "Normal", "1.5×": `speedLabel` in `model.ts`.
func speedLabel(_ speed: Double) -> String {
    speed == 1 ? "Normal" : "\(speed.formatted(.number.precision(.fractionLength(0...2))))×"
}

/// "12 min" or "45s": `sleepLeft` in `WatchMenu.tsx`.
func sleepLeft(_ state: SleepState) -> String {
    let left = state.remainingSecs ?? 0
    return left >= 120 ? "\(Int((Double(left) / 60).rounded())) min" : "\(left)s"
}

/// "1:02:03" or "4:05".
func clock(_ seconds: Double) -> String {
    let total = Int(max(0, seconds.isFinite ? seconds : 0))
    let h = total / 3600, m = total / 60 % 60, s = total % 60
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}
