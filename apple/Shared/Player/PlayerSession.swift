import CoreGraphics
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
        /// Typing a channel number with the remote or a finger (live only).
        case keypad
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

    // MARK: Zapping (`useZapper`)

    /// A channel as the player names it: enough to come back to it.
    struct Tuned: Equatable {
        let id: Int64
        let name: String
        let number: UInt32?
        var logo: String? = nil
    }

    /// The channel banner (Windows' `ChannelBanner`): the channel just tuned, what is on
    /// it now and next. `guide` is nil until the guide has answered.
    struct ChannelBanner: Equatable {
        var channel: Tuned
        var guide: NowNext?
    }

    let zap = zapRules()
    /// Digits typed so far, shown while a number is being entered.
    private(set) var digits = ""
    /// The channel before this one, for "last channel".
    private(set) var lastChannel: Tuned?
    /// Names the channel just tuned for a moment, without raising the controls: the next
    /// press of up or down zaps again.
    private(set) var channelBanner: ChannelBanner?
    /// Says something instead, for the same moment: that a number reached nothing.
    private(set) var banner: String?
    private var bannerUntil: Date?
    private var current: Tuned?
    /// What the viewer most recently asked to be on. A lookup that answers after they
    /// moved on is dropped, or it would put them back on a channel they had left.
    private var intent = 0
    private var digitTimer: Task<Void, Never>?

    // MARK: Sleep timer

    private(set) var sleep = SleepState(atUnix: nil, after: nil, remainingSecs: nil, warning: false)
    /// The timer ran out: the screen closes.
    private(set) var sleptAt: Date?

    // MARK: Daily limit (F-37)

    /// Today's viewing time ran out: playback has stopped and the screen closes.
    private(set) var timeUpAt: Date?
    private var lastViewingSample: TimeInterval = 0

    weak var controller: MPVPlayerController?
    let core: AuroraCore
    private let profileId: Int64
    private var ticker: Timer?

    init(item: PlayerItem, core: AuroraCore, profileId: Int64) {
        self.item = item
        self.core = core
        self.profileId = profileId
        sleep = core.sleepState()
        if case .channel(let id, let name) = item { current = Tuned(id: id, name: name, number: nil) }
    }

    var isLive: Bool {
        if case .channel = item { return true }
        return false
    }

    /// Films and catch-up always; a live channel while it has a rewind buffer.
    var seekable: Bool { !isLive || state.rewind != nil }

    /// How far behind live the viewer is; 0 at the edge, or with no buffer.
    var behindLive: Double { state.rewind?.behindSecs ?? 0 }

    /// Back to the newest moment, playing (Windows' Back to live, T).
    func backToLive() {
        controller?.backToLive()
        poke()
    }

    var episodeId: Int64? {
        if case .episode(let id, _, _) = item { return id }
        return nil
    }

    // MARK: Ticking

    func start() {
        nameCurrent()
        // Opening a channel is a tune like any other: Windows' zapper raises the banner
        // for the channel the player opens on too.
        if let current { showBanner(current) }
        ticker?.invalidate()
        ticker = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    func stop() {
        ticker?.invalidate()
        ticker = nil
        // Nothing is playing from here: the clock stops counting.
        let core = core
        Task.detached { _ = core.viewingSample(watching: false) }
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
        if let until = bannerUntil, Date() >= until { banner = nil; channelBanner = nil; bannerUntil = nil }
        #if os(iOS)
        let now = ProcessInfo.processInfo.systemUptime
        if now - lastPreviewTick >= Double(previewTickMs()) / 1000 {
            lastPreviewTick = now
            previewTick(now)
        }
        #endif
        viewingTick()
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

    /// The daily limit's clock, sampled once a second as Windows' heartbeat does: playing
    /// or buffering counts, paused or failed does not. The core keeps the time and says
    /// when it is nearly or wholly used up.
    private func viewingTick() {
        let now = ProcessInfo.processInfo.systemUptime
        guard now - lastViewingSample >= 1, timeUpAt == nil else { return }
        lastViewingSample = now
        let watching: Bool = switch status {
        case .playing?, .noPicture?: !state.paused
        case .opening?: true
        case .failed?, nil: false
        }
        // Off the main actor: writing the time down takes the library, which an import
        // may be holding.
        let core = core
        Task {
            let event = await Task.detached { core.viewingSample(watching: watching) }.value
            switch event {
            case .timeUp:
                guard timeUpAt == nil else { return }
                controller?.stop()
                timeUpAt = Date()
            case .warning(let secsLeft):
                say("\(max(1, Int((Double(secsLeft) / 60).rounded()))) minutes of viewing left today")
            case nil:
                break
            }
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
        // The file's chapters name its moments on the seek preview (H25).
        if state.loaded, chaptersFor != item.id, let controller {
            chaptersFor = item.id
            let asked = item.id
            controller.chapters { [weak self] marks in
                if self?.item.id == asked { self?.chapterMarks = marks }
            }
        }
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
        startOver()
        poke()
    }

    /// Forget what belonged to the item that was playing.
    private func startOver() {
        frames = [:]
        chapterMarks = []
        chaptersFor = nil
        aids = nil
        aidsStage = nil
        autoSkipped = []
        ourSeek = nil
        lastPosition = nil
        state = MPVPlayerController.PlaybackState()
        status = nil
    }

    // MARK: Zapping

    /// Put a channel on in place of this one.
    func tune(_ channel: Tuned) {
        intent += 1
        guard isLive else { return }
        if let current, current.id != channel.id { lastChannel = current }
        current = channel
        if case .channel(let id, _) = item, id == channel.id {
            // Already on: say so rather than open it again.
            showBanner(channel)
            return
        }
        item = .channel(id: channel.id, name: channel.name)
        startOver()
        showBanner(channel)
    }

    /// Channel up (`forward`) or down, through the list Windows zaps through.
    func step(forward: Bool) {
        guard isLive else { return }
        intent += 1
        let mine = intent
        let from = current?.id
        let core = core
        Task {
            do {
                let next = try await Task.detached { try core.channelStep(from: from, forward: forward) }.value
                guard mine == intent, let next else { return }
                tune(Tuned(next))
            } catch {
                guard mine == intent else { return }
                say(error.viewerMessage)
            }
        }
    }

    /// The channel the player opened on came with a name only: find its number, for the
    /// banner that "last channel" shows on the way back to it.
    private func nameCurrent() {
        guard let opened = current, opened.number == nil else { return }
        let core = core
        Task {
            let found = await Task.detached { try? core.channel(id: opened.id) }.value
            guard let found = found ?? nil else { return }
            let named = Tuned(found)
            if current?.id == named.id { current = named }
            if lastChannel?.id == named.id { lastChannel = named }
            if channelBanner?.channel.id == named.id { channelBanner?.channel = named }
        }
    }

    /// Back to the channel before this one (Windows' Backspace).
    func backToLast() {
        if let lastChannel { tune(lastChannel) }
    }

    /// A digit from the keypad. The number is tuned when the viewer stops typing.
    func typeDigit(_ digit: Int) {
        guard isLive else { return }
        digits = pushDigit(typed: digits, digit: String(digit))
        digitTimer?.cancel()
        let wait = UInt64(zap.digitTimeoutMs) * 1_000_000
        digitTimer = Task { [weak self] in
            try? await Task.sleep(nanoseconds: wait)
            guard !Task.isCancelled else { return }
            self?.commitDigits()
        }
    }

    /// Take back the last digit typed.
    func dropDigit() {
        guard !digits.isEmpty else { return }
        digits.removeLast()
        if digits.isEmpty { digitTimer?.cancel() }
    }

    /// Tune what has been typed now, without waiting.
    func commitDigits() {
        digitTimer?.cancel()
        let typed = digits
        digits = ""
        // The keypad has done its job: back to the picture, to see where it landed.
        if panel == .keypad { panel = nil }
        guard let number = UInt32(typed), number > 0 else { return }
        intent += 1
        let mine = intent
        let core = core
        Task {
            do {
                let hit = try await Task.detached { try core.channelByNumber(number: number) }.value
                guard mine == intent else { return }
                if let hit { tune(Tuned(hit)) } else { say("There is no channel \(number)") }
            } catch {
                guard mine == intent else { return }
                say(error.viewerMessage)
            }
        }
    }

    private func showBanner(_ channel: Tuned) {
        banner = nil
        let kept = channelBanner?.channel.id == channel.id ? channelBanner?.guide : nil
        channelBanner = ChannelBanner(channel: channel, guide: kept)
        // As long as Windows' banner stays (`showBanner`).
        bannerUntil = Date().addingTimeInterval(5)
        guard kept == nil else { return }
        let core = core
        Task {
            let answer = await Task.detached { try? core.nowNext(channelIds: [channel.id]) }.value
            guard let guide = answer?.first, channelBanner?.channel.id == channel.id else { return }
            channelBanner?.guide = guide
        }
    }

    private func say(_ text: String) {
        channelBanner = nil
        banner = text
        bannerUntil = Date().addingTimeInterval(5)
    }

    // MARK: Seek preview (`preview.rs`, Windows' `preview_tick` and `seek_preview`)

    /// The pictures kept for the seek preview, by the key the core handed out. Only the
    /// bubble reads them, when it asks, so nothing redraws as they arrive.
    @ObservationIgnored private var frames: [String: CGImage] = [:]
    @ObservationIgnored private var grabbing = false
    @ObservationIgnored private var lastPreviewTick = 0.0
    @ObservationIgnored private var chapterMarks: [ChapterMark] = []
    @ObservationIgnored private var chaptersFor: String?

    /// What is playing, as the preview names it.
    private var previewTitle: PreviewTitle? {
        switch item {
        case .channel(let id, _): .channel(id: id)
        case .movie(let id, _, _): .movie(id: id)
        case .episode(let id, _, _): .episode(id: id)
        case .catchup(let channel, _, let start, let stop): .catchup(channel: channel, start: start, stop: stop)
        }
    }

    private var previewSample: PreviewSample {
        PreviewSample(
            playing: state.loaded && !state.paused && status == .playing,
            // `.noPicture` is sound alone: nothing to take a picture of.
            hasPicture: status == .playing,
            positionSecs: state.position,
            durationSecs: state.duration,
            speed: state.speed,
            isLive: isLive,
            window: state.rewind
        )
    }

    /// Once a second: take a picture when the core says one is due.
    private func previewTick(_ now: Double) {
        guard !grabbing, let controller else { return }
        let title = previewTitle
        guard core.previewDue(title: title, sample: previewSample, now: now) else { return }
        grabbing = true
        controller.grabFrame(maxWidth: Int(previewMaxWidth())) { [weak self] image, at in
            guard let self else { return }
            grabbing = false
            guard let image, let kept = core.previewKeep(title: title, atSecs: at, nowAt: state.position)
            else { return }
            frames[kept.key] = image
            let held = Set(kept.kept)
            frames = frames.filter { held.contains($0.key) }
        }
    }

    /// What the bar would show at `position`: the picture nearest it, if one was taken,
    /// and what that moment was.
    func seekPreview(at position: Double) async -> (SeekPreview, CGImage?)? {
        var channelId: Int64?
        if case .channel(let id, _) = item { channelId = id }
        let (core, title, sample, chapters, episodeId) = (core, previewTitle, previewSample, chapterMarks, episodeId)
        let answer = await Task.detached {
            try? core.seekPreview(title: title, positionSecs: position, sample: sample,
                                  chapters: chapters, channelId: channelId, episodeId: episodeId)
        }.value
        guard let preview = answer else { return nil }
        return (preview, preview.image.flatMap { frames[$0] })
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

extension PlayerSession.Tuned {
    init(_ channel: Channel) {
        self.init(id: channel.id, name: channel.name, number: channel.number, logo: channel.logo)
    }
}
