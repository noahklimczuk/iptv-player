import SwiftUI

/// Where the remote's focus can be in the player (tvOS).
enum PlayerFocus: Hashable {
    case backdrop, back, playPause, forward, subtitles, audio, picture, close
    case channelDown, channelUp, lastChannel, keypad, backToLive
    case skip, upNextPlay, upNextCancel, sleepKeep

    /// The transport and menu buttons: focus on one of these is the viewer using the
    /// controls, which keeps them up. Skip, Up Next and the sleep warning stand alone.
    var isChrome: Bool {
        switch self {
        case .back, .playPause, .forward, .subtitles, .audio, .picture, .close,
             .channelDown, .channelUp, .lastChannel, .keypad, .backToLive: true
        case .backdrop, .skip, .upNextPlay, .upNextCancel, .sleepKeep: false
        }
    }

    var isCorner: Bool {
        switch self {
        case .skip, .upNextPlay, .upNextCancel, .sleepKeep: true
        default: false
        }
    }
}

/// A button that draws nothing of its own: the tvOS player's backdrop.
struct BareButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { configuration.label }
}

/// Everything over the picture: the Apple counterpart of `src-ui` `PlayerOverlay`.
struct PlayerOverlay: View {
    @Bindable var session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let close: () -> Void

    #if os(tvOS)
    private let edge: CGFloat = 60
    #else
    private let edge: CGFloat = 16
    #endif

    var body: some View {
        ZStack {
            if case .failed(let message) = session.status {
                failed(message)
            } else {
                if session.panel == nil && session.controlsVisible {
                    PlayerChrome(session: session, focus: focus, close: close)
                        .transition(.opacity)
                } else if session.panel == nil, session.seekHintUntil != nil {
                    // The remote jumped with the controls hidden: just where it landed.
                    VStack {
                        Spacer()
                        ProgressRow(session: session, interactive: false)
                    }
                    .padding(edge)
                    .allowsHitTesting(false)
                }
                status
                zapping
                corner
                if let panel = session.panel {
                    WatchPanel(session: session, tab: panel)
                        .transition(.move(edge: .trailing))
                }
                if session.sleep.warning {
                    SleepWarning(session: session, focus: focus)
                }
            }
        }
        // Over a picture, whatever the rest of the app is set to: a light menu on a film
        // is a lamp switched on in the dark.
        .environment(\.colorScheme, .dark)
        .animation(.easeInOut(duration: 0.2), value: session.controlsVisible)
        .animation(.easeInOut(duration: 0.2), value: session.panel)
        #if os(tvOS)
        // Keep the controls up while the remote moves through them. Skip and Up Next
        // don't bring them up: those stand alone over the picture.
        .onChange(of: focus.wrappedValue) { _, new in
            if new?.isChrome == true { session.poke() }
        }
        .onChange(of: session.controlsVisible) { _, visible in
            if visible, focus.wrappedValue?.isCorner != true {
                // Next turn of the run loop: the buttons appear in this same update,
                // and focus given to one that isn't there yet goes to the first one.
                DispatchQueue.main.async { focus.wrappedValue = .playPause }
            } else if !visible, focus.wrappedValue?.isCorner != true {
                // Fading controls leave a focused Skip or Up Next where it is.
                focus.wrappedValue = .backdrop
            }
        }
        // Skip and Up Next take the remote's focus as they appear, as a TV's do, and
        // give it back when they go.
        .onChange(of: session.activeMarker?.startSecs) { _, start in
            if start != nil, session.panel == nil, !session.upNextVisible {
                focus.wrappedValue = .skip
            } else if start == nil, focus.wrappedValue == .skip {
                restFocus()
            }
        }
        .onChange(of: session.upNextVisible) { _, visible in
            if visible, session.panel == nil {
                focus.wrappedValue = .upNextPlay
            } else if !visible, focus.wrappedValue == .upNextPlay || focus.wrappedValue == .upNextCancel {
                if session.activeMarker != nil { focus.wrappedValue = .skip } else { restFocus() }
            }
        }
        // A closed panel hands focus back to the button that opened it (the panel's
        // own tabs can move it to another, so the tab it closed on decides).
        .onChange(of: session.panel) { old, new in
            guard new == nil, let old, session.controlsVisible else { return }
            // Audio's button is missing with one track: Subtitles is beside where it was.
            let opener: PlayerFocus = old == .audio && session.state.audioTracks.count <= 1 ? .subtitles : old.opener
            DispatchQueue.main.async { focus.wrappedValue = opener }
        }
        .onChange(of: session.sleep.warning) { _, warning in
            if warning {
                focus.wrappedValue = .sleepKeep
            } else if focus.wrappedValue == .sleepKeep {
                restFocus()
            }
        }
        #endif
    }

    #if os(tvOS)
    /// Where focus goes when what held it disappears.
    private func restFocus() {
        focus.wrappedValue = session.controlsVisible ? .playPause : .backdrop
    }
    #endif

    /// Opening, or sound without a picture.
    @ViewBuilder private var status: some View {
        VStack {
            HStack {
                switch session.status {
                // The channel banner says what is opening, and has the top for its moment.
                case .opening(let source, let total) where !session.controlsVisible && session.channelBanner == nil:
                    Label(total > 1
                          ? "Opening \(session.item.title) (source \(source) of \(total))"
                          : "Opening \(session.item.title)",
                          systemImage: "dot.radiowaves.left.and.right")
                        .padding()
                        .background(.ultraThinMaterial, in: Capsule())
                case .noPicture(let message) where !session.controlsVisible && session.channelBanner == nil:
                    Label(message, systemImage: "speaker.wave.2")
                        .padding()
                        .background(.ultraThinMaterial, in: Capsule())
                default:
                    EmptyView()
                }
                Spacer()
            }
            Spacer()
        }
        .padding(edge)
        .allowsHitTesting(false)
    }

    /// The number being typed, top right; the channel just tuned, top centre, clear of
    /// the controls (Windows' `ChannelBanner` over the player); and anything said about a
    /// zap, bottom left. None of them raises the controls.
    private var zapping: some View {
        VStack {
            HStack(alignment: .top) {
                Spacer()
                if let banner = session.channelBanner, session.panel == nil {
                    ChannelBannerView(banner: banner)
                        // Below the title row while the controls are up, clear of it.
                        .padding(.top, session.controlsVisible ? titleHeight : 0)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
                Spacer()
            }
            .overlay(alignment: .topTrailing) {
                if !session.digits.isEmpty {
                    Text(session.digits)
                        .font(.system(size: digitSize, weight: .bold, design: .rounded))
                        .monospacedDigit()
                        .padding(.horizontal, 20).padding(.vertical, 8)
                        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
                        .accessibilityLabel("Channel \(session.digits)")
                }
            }
            Spacer()
            HStack {
                if let banner = session.banner, session.panel == nil {
                    Text(banner)
                        .font(.title3.bold())
                        .lineLimit(2)
                        .padding(.horizontal, 18).padding(.vertical, 10)
                        .background(.ultraThinMaterial, in: Capsule())
                        .transition(.opacity)
                }
                Spacer()
            }
            .padding(.bottom, chromeHeight)
        }
        .padding(edge)
        .allowsHitTesting(false)
        .animation(.easeInOut(duration: 0.2), value: session.banner)
        .animation(.easeOut(duration: 0.22), value: session.channelBanner?.channel)
        .animation(.easeInOut(duration: 0.2), value: session.controlsVisible)
    }

    private var digitSize: CGFloat {
        #if os(tvOS)
        72
        #else
        40
        #endif
    }

    /// Skip and Up Next, bottom right, one stack above where the controls sit (`App.tsx`).
    /// Both can be live during the credits, so they queue rather than overlap. The stack
    /// stays put when the controls fade: one that dropped into their place would take a
    /// tap meant for a control that had just gone.
    private var corner: some View {
        VStack(alignment: .trailing, spacing: 12) {
            Spacer()
            if session.panel == nil {
                if session.upNextVisible, let next = session.aids?.nextEpisode {
                    UpNextCard(session: session, episode: next, focus: focus)
                }
                if let marker = session.activeMarker {
                    SkipButton(session: session, marker: marker, focus: focus)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
        .padding(edge)
        .padding(.bottom, chromeHeight)
    }

    /// The controls' title row, which the channel banner sits below.
    private var titleHeight: CGFloat {
        #if os(tvOS)
        90
        #else
        52
        #endif
    }

    private var chromeHeight: CGFloat {
        #if os(tvOS)
        220
        #else
        130
        #endif
    }

    private func failed(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Could not play \(session.item.title)").font(.headline)
            Text(message)
            HStack {
                // A dead channel isn't the end of watching: the next one is a press away.
                if session.isLive {
                    Button("Channel down", systemImage: "chevron.down") { session.step(forward: false) }
                    Button("Channel up", systemImage: "chevron.up") { session.step(forward: true) }
                }
                Button("Close", action: close)
            }
            #if os(iOS)
            .buttonStyle(.bordered)
            #endif
        }
        .padding()
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
        .padding(edge)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

// MARK: Controls

/// Title on top, progress and buttons below (`Dock.tsx`).
private struct PlayerChrome: View {
    @Environment(AppModel.self) private var model
    @Bindable var session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(session.item.title).font(.title3.bold()).lineLimit(1)
                    switch session.status {
                    case .noPicture(let message):
                        Label(message, systemImage: "speaker.wave.2").font(.caption)
                    case .opening(let source, let total):
                        Label(total > 1 ? "Opening (source \(source) of \(total))…" : "Opening…",
                              systemImage: "dot.radiowaves.left.and.right")
                            .font(.caption)
                    default:
                        EmptyView()
                    }
                }
                Spacer()
                // Live only: a film is already a file on the provider's disk.
                if session.isLive {
                    Button("Record what's on", systemImage: "record.circle") { session.recordNow(model: model) }
                        .labelStyle(.iconOnly)
                        .font(.title3)
                        #if os(iOS)
                        .padding(8)
                        .background(.ultraThinMaterial, in: Circle())
                        #endif
                }
                #if os(iOS)
                Button("Close", systemImage: "xmark", action: close)
                    .labelStyle(.iconOnly)
                    .font(.title3)
                    .padding(8)
                    .background(.ultraThinMaterial, in: Circle())
                #endif
            }
            Spacer()
            if session.isLive, let guide = session.liveGuide {
                LiveCards(guide: guide) { session.recordNext(model: model) }
                    .padding(.bottom, 8)
            }
            HStack(spacing: spacing) {
                ProgressRow(session: session, interactive: true)
                if session.behindLive > 0 {
                    // Windows' Back to live (T): only while behind it.
                    Button { session.backToLive() } label: {
                        Label("Back to live", systemImage: "dot.radiowaves.left.and.right")
                            .lineLimit(1)
                    }
                    .font(.callout)
                    .focused(focus, equals: .backToLive)
                    #if os(iOS)
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
                    #endif
                } else if session.isLive, let last = session.lastChannel {
                    Spacer()
                    // Windows' Backspace: the channel before this one.
                    Button { session.backToLast() } label: {
                        Label("Back to \(last.name)", systemImage: "arrow.uturn.backward")
                            .lineLimit(1)
                    }
                    .font(.callout)
                    .focused(focus, equals: .lastChannel)
                    #if os(iOS)
                    .buttonStyle(.bordered)
                    #endif
                }
            }
            // A rewindable channel adds two buttons to a row already holding the channel
            // controls; where they don't fit (an iPhone upright) the bar still seeks.
            ViewThatFits(in: .horizontal) {
                transportRow(skips: true)
                transportRow(skips: false)
            }
            .labelStyle(.iconOnly)
        }
        .padding(edge)
        .foregroundStyle(.white)
        .task(id: session.item) { await session.loadLiveGuide() }
        .background {
            LinearGradient(colors: [.black.opacity(0.7), .clear, .clear, .black.opacity(0.8)],
                           startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea()
                .allowsHitTesting(false)
        }
    }

    private func transportRow(skips: Bool) -> some View {
            HStack(spacing: spacing) {
                if skips, session.seekable {
                    transport("Back 10 seconds", "gobackward.10", .back) { session.seek(by: -10) }
                }
                if session.isLive {
                    transport("Channel down", "chevron.down", .channelDown) { session.step(forward: false) }
                }
                transport(session.state.paused ? "Play" : "Pause",
                          session.state.paused ? "play.fill" : "pause.fill", .playPause) { session.togglePause() }
                if skips, session.seekable {
                    transport("Forward 30 seconds", "goforward.30", .forward) { session.seek(by: 30) }
                }
                if session.isLive {
                    transport("Channel up", "chevron.up", .channelUp) { session.step(forward: true) }
                }
                Spacer()
                if session.isLive {
                    menuButton("Channel number", value: nil, icon: "number", tab: .keypad)
                }
                menuButton("Subtitles", value: subtitleChoice, icon: "captions.bubble", tab: .subtitles)
                if session.state.audioTracks.count > 1 {
                    menuButton("Audio", value: audioChoice, icon: "waveform", tab: .audio)
                }
                menuButton("Picture", value: nil, icon: "slider.horizontal.3", tab: .picture)
            }
            .labelStyle(.iconOnly)
    }

    #if os(tvOS)
    private let edge: CGFloat = 60
    private let spacing: CGFloat = 24
    #else
    private let edge: CGFloat = 16
    private let spacing: CGFloat = 18
    #endif

    /// What each menu is set to now, in words (`currentChoices`, C13).
    private var subtitleChoice: String {
        session.state.subtitleTracks.first { $0.id == session.state.subtitleId }?.name ?? "Off"
    }

    private var audioChoice: String? {
        session.state.audioTracks.first { $0.id == session.state.audioId }?.name
    }

    private func transport(_ title: String, _ icon: String, _ id: PlayerFocus,
                           action: @escaping () -> Void) -> some View {
        Button(title, systemImage: icon, action: action)
            .font(.title2)
            .focused(focus, equals: id)
            #if os(iOS)
            .buttonStyle(.plain)
            .padding(6)
            #endif
    }

    private func menuButton(_ title: String, value: String?, icon: String,
                            tab: PlayerSession.Panel) -> some View {
        Button {
            session.panel = tab
        } label: {
            Label(value.map { "\(title): \($0)" } ?? title, systemImage: icon)
        }
        .accessibilityLabel(value.map { "\(title): \($0)" } ?? title)
        .font(.title3)
        .focused(focus, equals: tab.opener)
        #if os(iOS)
        .buttonStyle(.plain)
        .padding(6)
        #endif
    }
}

/// On now at the bottom left; next at the bottom right, with a Record button (Windows'
/// `LiveCards`).
private struct LiveCards: View {
    let guide: NowNext
    let recordNext: () -> Void

    var body: some View {
        HStack(alignment: .bottom, spacing: 16) {
            if let on = guide.now {
                VStack(alignment: .leading, spacing: 4) {
                    Text("On now · \(Format.clock(on.start))–\(Format.clock(on.stop))")
                        .font(.caption.bold()).foregroundStyle(.white.opacity(0.75))
                    Text(on.title).font(.title3.weight(.heavy)).lineLimit(1)
                    if let text = on.description ?? on.subTitle {
                        Text(text).font(.caption).lineLimit(2).foregroundStyle(.white.opacity(0.8))
                    }
                }
                .padding(12)
                .frame(maxWidth: 420, alignment: .leading)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
            }
            Spacer(minLength: 0)
            if let next = guide.next {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("NEXT · \(Format.clock(next.start))")
                            .font(.caption.bold()).foregroundStyle(.white.opacity(0.75))
                        Text(next.title).font(.callout.bold()).lineLimit(1)
                    }
                    Button("Record", systemImage: "record.circle", action: recordNext)
                        .accessibilityLabel("Record \(next.title)")
                        #if os(iOS)
                        .buttonStyle(.bordered)
                        #endif
                }
                .padding(12)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
                .accessibilityElement(children: .contain)
            }
        }
    }
}

/// The progress bar and its times, or "Live".
private struct ProgressRow: View {
    @Bindable var session: PlayerSession
    let interactive: Bool
    /// While a finger drags the bar: where it is, so the bar doesn't jump back.
    @State private var dragging: Double?
    /// What the seek preview says about where the finger is.
    @State private var preview: Shown?

    struct Shown {
        let preview: SeekPreview
        let image: CGImage?
    }

    var body: some View {
        if session.isLive, let window = session.state.rewind {
            timeshift(window)
        } else if session.isLive {
            liveBadge
        } else if session.state.duration > 0 {
            HStack {
                Text(clock(dragging ?? session.state.position)).monospacedDigit()
                bar
                Text("-" + clock(session.state.duration - (dragging ?? session.state.position))).monospacedDigit()
            }
            .font(.caption)
            .foregroundStyle(.white)
        } else if session.state.loaded {
            // The stream doesn't say how long it is: just where it is.
            Text(clock(session.state.position)).font(.caption).monospacedDigit().foregroundStyle(.white)
        }
    }

    private var liveBadge: some View {
        Label("Live", systemImage: "dot.radiowaves.left.and.right")
            .font(.caption.bold())
            .padding(.horizontal, 10).padding(.vertical, 4)
            .background(.red, in: Capsule())
            .foregroundStyle(.white)
    }

    /// Live with a buffer (`Scrub.tsx`'s timeshift timeline): how far behind live, how
    /// much is held, and a bar from the oldest moment held to live.
    private func timeshift(_ w: RewindWindow) -> some View {
        // While dragging, the readout says where the finger is, as Windows' preview does.
        let behind = dragging.map { max(0, w.liveSecs - $0) } ?? w.behindSecs
        return VStack(alignment: .leading, spacing: 6) {
            HStack {
                if behind > liveEdgeSecs {
                    Text("−" + clock(behind))
                        .monospacedDigit()
                        .accessibilityLabel("\(clock(behind)) behind live")
                } else {
                    liveBadge
                }
                Spacer()
                Text(w.heldSecs > 0 ? "\(clock(w.heldSecs)) buffered" : "Buffering…")
                    .monospacedDigit()
                    .foregroundStyle(.white.opacity(0.7))
            }
            .font(.caption)
            timeshiftBar(w)
        }
        .foregroundStyle(.white)
    }

    /// Windows' `LIVE_EDGE_SECS`, which the core's `behind_secs` already uses.
    private let liveEdgeSecs = 5.0

    @ViewBuilder private func timeshiftBar(_ w: RewindWindow) -> some View {
        let upper = max(w.liveSecs, w.startSecs + 1)
        #if os(iOS)
        if interactive {
            Slider(
                value: Binding(get: { dragging ?? min(max(w.positionSecs, w.startSecs), upper) },
                               // A hand on the bar keeps the controls up.
                               set: { dragging = $0; session.poke() }),
                in: w.startSecs...upper,
                onEditingChanged: { editing in
                    if editing {
                        session.poke()
                    } else if let to = dragging {
                        session.seek(to: to)
                        dragging = nil
                    }
                }
            )
            .tint(.red)
            .background { Capsule().fill(.white.opacity(0.3)).frame(height: 4) }
            // Above the readout row, which sits over this bar.
            .overlay { bubble(in: w.startSecs...upper, lift: 34) { secs, p in
                // `Scrub.tsx`'s timeshift words.
                let back = max(0, w.liveSecs - secs)
                return (back <= liveEdgeSecs ? "Live" : "−" + clock(back),
                        [p?.clockUnix.map(wallClock), p?.programme, p?.label])
            } }
            .accessibilityLabel("Timeshift")
        } else {
            ProgressView(value: min(max(0, w.positionSecs - w.startSecs), upper - w.startSecs),
                         total: upper - w.startSecs)
                .tint(.red)
        }
        #else
        ProgressView(value: min(max(0, w.positionSecs - w.startSecs), upper - w.startSecs),
                     total: upper - w.startSecs)
            .tint(.red)
        #endif
    }

    #if os(iOS)
    /// The seek preview over the bar while a finger drags it (`Scrub.tsx`'s
    /// `SeekBubble`): the picture nearest that moment, if one was taken, and what it was.
    private func bubble(in range: ClosedRange<Double>, lift: CGFloat,
                        words: @escaping (Double, SeekPreview?) -> (String, [String?])) -> some View {
        GeometryReader { geo in
            if let at = dragging {
                let span = range.upperBound - range.lowerBound
                let x = span > 0 ? (at - range.lowerBound) / span * geo.size.width : 0
                let (time, lines) = words(at, preview?.preview)
                // Kept inside the bar, so the bubble at either end is not cut off.
                let half = SeekBubble.width / 2
                let left = geo.size.width > SeekBubble.width ? min(max(x, half), geo.size.width - half)
                                                             : geo.size.width / 2
                // A point-sized anchor above the bar the bubble grows up from, centred on it.
                SeekBubble(time: time, lines: lines, image: preview?.image)
                    .fixedSize()
                    .frame(width: 0, height: 0, alignment: .bottom)
                    .offset(x: left, y: -lift)
            }
        }
        .allowsHitTesting(false)
        // One question at a time, asked again while the picture is still coming (Windows'
        // `useSeekPreview`, 150 ms).
        .task(id: dragging) {
            guard let at = dragging else {
                preview = nil
                return
            }
            while !Task.isCancelled {
                guard let answer = await session.seekPreview(at: at), !Task.isCancelled else { return }
                preview = Shown(preview: answer.0, image: answer.1)
                guard answer.0.pending else { return }
                try? await Task.sleep(for: .milliseconds(150))
            }
        }
    }
    #endif

    @ViewBuilder private var bar: some View {
        #if os(iOS)
        if interactive {
            Slider(
                value: Binding(get: { dragging ?? session.state.position },
                               set: { dragging = $0; session.poke() }),
                in: 0...max(session.state.duration, 1),
                onEditingChanged: { editing in
                    if editing {
                        session.poke()
                    } else if let to = dragging {
                        session.seek(to: to)
                        dragging = nil
                    }
                }
            )
            .tint(.white)
            // The unplayed part of the track is drawn dark, which vanishes on black.
            .background { Capsule().fill(.white.opacity(0.3)).frame(height: 4) }
            .overlay { bubble(in: 0...max(session.state.duration, 1), lift: 10) { secs, p in
                (clock(secs), [p?.label])
            } }
            .accessibilityLabel("Position")
        } else {
            ProgressView(value: min(session.state.position, session.state.duration), total: session.state.duration)
                .tint(.white)
        }
        #else
        ProgressView(value: min(session.state.position, session.state.duration), total: session.state.duration)
            .tint(.white)
        #endif
    }
}

// MARK: Watch panel

/// Subtitles, Audio and Picture (`WatchMenu.tsx`): the menus, as a side panel.
private struct WatchPanel: View {
    @Bindable var session: PlayerSession
    let tab: PlayerSession.Panel
    @State private var sleepError: String?

    var body: some View {
        HStack {
            Spacer()
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    if tab == .keypad {
                        Text("Channel number").font(.headline)
                        Spacer()
                    } else {
                        Picker("Menu", selection: Binding(get: { tab }, set: { session.panel = $0 })) {
                            Text("Subtitles").tag(PlayerSession.Panel.subtitles)
                            Text("Audio").tag(PlayerSession.Panel.audio)
                            Text("Picture").tag(PlayerSession.Panel.picture)
                        }
                        .pickerStyle(.segmented)
                    }
                    #if os(iOS)
                    Button("Close", systemImage: "xmark") { session.panel = nil }
                        .labelStyle(.iconOnly)
                    #endif
                }
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        switch tab {
                        case .subtitles: subtitles
                        case .audio: audio
                        case .picture: picture
                        case .keypad: Keypad(session: session)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(padding)
            .frame(width: width)
            .frame(maxHeight: .infinity)
            // The panel's glass runs to the edges; what is on it stays clear of the
            // status bar and the Dynamic Island.
            .background { Rectangle().fill(.regularMaterial).ignoresSafeArea() }
        }
    }

    #if os(tvOS)
    private let width: CGFloat = 720
    private let padding: CGFloat = 50
    #else
    private let width: CGFloat = 340
    private let padding: CGFloat = 20
    #endif

    @ViewBuilder private var subtitles: some View {
        Choice(label: "Off", active: session.state.subtitleId == nil) { session.setSubtitleTrack(nil) }
        ForEach(session.state.subtitleTracks) { track in
            Choice(label: track.name, detail: track.detail, active: session.state.subtitleId == track.id) {
                session.setSubtitleTrack(track.id)
            }
        }
    }

    @ViewBuilder private var audio: some View {
        ForEach(session.state.audioTracks) { track in
            Choice(label: track.name, detail: track.detail, active: session.state.audioId == track.id) {
                session.setAudioTrack(track.id)
            }
        }
        if session.state.audioTracks.count <= 1 {
            Text("This stream has one audio track.").foregroundStyle(.secondary)
        }
    }

    @ViewBuilder private var picture: some View {
        // Live arrives at the rate it arrives; faster would only run out of buffer.
        if !session.isLive {
            Heading("Speed")
            Chips {
                ForEach(session.menus.speeds, id: \.self) { speed in
                    Chip(label: speedLabel(speed), active: session.state.speed == speed) { session.setSpeed(speed) }
                }
            }
        }
        Heading("Aspect ratio")
        ForEach(session.menus.aspects, id: \.label) { choice in
            Choice(label: choice.label, detail: choice.detail, active: session.aspect == choice.aspect) {
                session.setAspect(choice.aspect)
            }
        }
        Heading("Sleep timer")
        if session.sleep.atUnix != nil {
            // Windows says "Stopping": on Apple the timer only ever stops playback.
            Text("Stopping in \(sleepLeft(session.sleep))")
            Chips {
                Chip(label: "+\(session.menus.sleepExtendMinutes) min", active: false) { session.extendSleep() }
                Chip(label: "Off", active: false) { session.cancelSleep() }
            }
        } else {
            Chips {
                ForEach(session.menus.sleepMinutes, id: \.self) { minutes in
                    Chip(label: "\(minutes) min", active: false) { setSleep(.minutes(minutes: minutes)) }
                }
                // Only what can be answered: live has a programme, on demand a runtime.
                if session.isLive {
                    Chip(label: "End of programme", active: false) { setSleep(.endOfProgramme) }
                } else if session.state.duration > 0 {
                    Chip(label: endLabel, active: false) { setSleep(.endOfEpisode) }
                }
            }
        }
        if let sleepError {
            Text(sleepError).font(.caption).foregroundStyle(.red)
        }
    }

    private var endLabel: String {
        switch session.item {
        case .episode: "End of episode"
        case .catchup, .recording: "End of programme"
        default: "End of film"
        }
    }

    private func setSleep(_ after: SleepAfter) {
        do {
            try session.setSleep(after)
            sleepError = nil
        } catch {
            sleepError = error.viewerMessage
        }
    }
}

/// Digits for a channel number (Windows' number keys). The number is tuned when typing
/// stops, as on Windows; Go tunes it at once.
private struct Keypad: View {
    let session: PlayerSession

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(session.digits.isEmpty ? "–" : session.digits)
                .font(.system(size: 44, weight: .bold, design: .rounded))
                .monospacedDigit()
                .frame(maxWidth: .infinity, alignment: .center)
                .accessibilityLabel(session.digits.isEmpty ? "No number typed" : "Typed \(session.digits)")
            Grid(horizontalSpacing: spacing, verticalSpacing: spacing) {
                ForEach([[1, 2, 3], [4, 5, 6], [7, 8, 9]], id: \.self) { row in
                    GridRow {
                        ForEach(row, id: \.self) { digit($0) }
                    }
                }
                GridRow {
                    key("Delete", systemImage: "delete.left") { session.dropDigit() }
                        .disabled(session.digits.isEmpty)
                    digit(0)
                    key("Go", systemImage: "return") { session.commitDigits() }
                    .disabled(session.digits.isEmpty)
                }
            }
            Text("Up to \(session.zap.maxDigits) digits. The channel changes when you stop typing.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    #if os(tvOS)
    private let spacing: CGFloat = 20
    #else
    private let spacing: CGFloat = 10
    #endif

    private func digit(_ d: Int) -> some View {
        key("\(d)", systemImage: nil) { session.typeDigit(d) }
    }

    private func key(_ title: String, systemImage: String?, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Group {
                if let systemImage {
                    Image(systemName: systemImage).accessibilityLabel(title)
                } else {
                    Text(title).monospacedDigit()
                }
            }
            .font(.title2)
            .frame(maxWidth: .infinity, minHeight: 44)
        }
        #if os(iOS)
        .buttonStyle(.bordered)
        #endif
    }
}

private struct Heading: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(.headline).padding(.top, 8)
    }
}

private struct Choice: View {
    let label: String
    var detail: String = ""
    let active: Bool
    let pick: () -> Void

    var body: some View {
        Button(action: pick) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(label)
                    if !detail.isEmpty {
                        Text(detail).font(.caption).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if active { Image(systemName: "checkmark") }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .accessibilityAddTraits(active ? .isSelected : [])
        #if os(iOS)
        .buttonStyle(.plain)
        .padding(.vertical, 6)
        #endif
    }
}

private struct Chips<Content: View>: View {
    @ViewBuilder let content: Content
    var body: some View {
        FlowLayout(spacing: 8) { content }
    }
}

private struct Chip: View {
    let label: String
    let active: Bool
    let pick: () -> Void

    var body: some View {
        Button(action: pick) { Text(label) }
            .accessibilityAddTraits(active ? .isSelected : [])
            #if os(iOS)
            .buttonStyle(.bordered)
            .tint(active ? .accentColor : .secondary)
            #else
            .overlay(alignment: .topTrailing) {
                if active { Image(systemName: "checkmark.circle.fill").offset(x: 8, y: -8) }
            }
            #endif
    }
}

/// Wraps chips onto as many lines as they need, each centred on its line.
struct FlowLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(width: proposal.width ?? .infinity, subviews: subviews)
        return CGSize(width: proposal.width ?? rows.map(\.width).max() ?? 0,
                      height: rows.last.map { $0.y + $0.height } ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for row in arrange(width: bounds.width, subviews: subviews) {
            var x = bounds.minX
            for index in row.items {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: bounds.minY + row.y + (row.height - size.height) / 2),
                                      proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
        }
    }

    private struct Row { var items: [Int] = []; var width: CGFloat = 0; var y: CGFloat = 0; var height: CGFloat = 0 }

    private func arrange(width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = [Row()]
        for (index, subview) in subviews.enumerated() {
            let size = subview.sizeThatFits(.unspecified)
            if !rows[rows.count - 1].items.isEmpty, rows[rows.count - 1].width + spacing + size.width > width {
                let last = rows[rows.count - 1]
                rows.append(Row(y: last.y + last.height + spacing))
            }
            var row = rows[rows.count - 1]
            row.width += (row.items.isEmpty ? 0 : spacing) + size.width
            row.height = max(row.height, size.height)
            row.items.append(index)
            rows[rows.count - 1] = row
        }
        return rows
    }
}

// MARK: Skip, Up Next, sleep

/// Skip Intro / Recap / Credits (C22). A marker learned from the viewer's own skips
/// says so.
private struct SkipButton: View {
    let session: PlayerSession
    let marker: SkipMarker
    var focus: FocusState<PlayerFocus?>.Binding

    var body: some View {
        Button { session.skip(marker) } label: {
            VStack(alignment: .leading, spacing: 2) {
                Label(marker.label, systemImage: "forward.end.fill").font(.headline)
                if marker.source == .learned {
                    Text("Learned from your skips").font(.caption2)
                }
            }
            .padding(.horizontal, 6)
        }
        .focused(focus, equals: .skip)
        .accessibilityLabel(marker.label)
        .accessibilityHint(marker.source == .learned ? "Learned from your skips in this show" : "")
        #if os(iOS)
        .buttonStyle(.borderedProminent)
        .tint(.white)
        .foregroundStyle(.black)
        #endif
    }
}

/// Up Next (C23): Play now, Cancel, and a countdown when the show plays on by itself.
private struct UpNextCard: View {
    let session: PlayerSession
    let episode: Episode
    var focus: FocusState<PlayerFocus?>.Binding

    private var autoplay: Bool { session.aids?.prefs.autoplayNext == true }

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.25)) { _ in
            let left = session.upNextLeft
            let total = Double(session.menus.upNextCountdownSecs)
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top, spacing: 12) {
                    if let still = episode.still, let url = URL(string: still) {
                        AsyncImage(url: url) { $0.resizable().scaledToFill() } placeholder: { Color.gray.opacity(0.3) }
                            .frame(width: stillWidth, height: stillWidth * 9 / 16)
                            .clipShape(RoundedRectangle(cornerRadius: 8))
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        Text(autoplay ? "Up next in \(left)s" : "Up next")
                            .font(.caption.bold()).foregroundStyle(.secondary)
                        Text(episode.title ?? "Episode \(episode.episode)").font(.headline).lineLimit(2)
                        Text("S\(episode.season) E\(episode.episode)"
                             + (episode.runtimeMins.map { " · \(runtime($0))" } ?? ""))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                HStack {
                    Button("Play now", systemImage: "play.fill") { session.playNext() }
                        .focused(focus, equals: .upNextPlay)
                        #if os(iOS)
                        .buttonStyle(.borderedProminent)
                        #endif
                    Button("Cancel") { session.dismissUpNext() }
                        .focused(focus, equals: .upNextCancel)
                        #if os(iOS)
                        .buttonStyle(.bordered)
                        #endif
                }
                if autoplay {
                    ProgressView(value: total - Double(left), total: total)
                        .accessibilityLabel("Countdown")
                }
            }
            .padding()
            .frame(width: cardWidth, alignment: .leading)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16))
        }
    }

    #if os(tvOS)
    private let cardWidth: CGFloat = 640
    private let stillWidth: CGFloat = 220
    #else
    private let cardWidth: CGFloat = 340
    private let stillWidth: CGFloat = 110
    #endif

    private func runtime(_ minutes: UInt32) -> String {
        minutes >= 60 ? "\(minutes / 60)h \(minutes % 60)m" : "\(minutes)m"
    }
}

/// The last minute of the sleep timer (H20): Keep watching or Cancel.
private struct SleepWarning: View {
    let session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding

    var body: some View {
        VStack {
            HStack(spacing: 16) {
                Label("Stopping in \(session.sleep.remainingSecs ?? 0)s", systemImage: "moon.zzz")
                Button("Keep watching", systemImage: "play.fill") { session.extendSleep() }
                    .accessibilityLabel("Keep watching for another \(session.menus.sleepExtendMinutes) minutes")
                    .focused(focus, equals: .sleepKeep)
                Button("Cancel", systemImage: "xmark") { session.cancelSleep() }
                    .accessibilityLabel("Cancel the sleep timer")
            }
            #if os(iOS)
            .buttonStyle(.bordered)
            #endif
            .padding()
            .background(.regularMaterial, in: Capsule())
            Spacer()
        }
        .padding(.top, 24)
    }
}

extension PlayerSession.Panel {
    /// The button in the controls that opens this panel.
    var opener: PlayerFocus {
        switch self {
        case .subtitles: .subtitles
        case .audio: .audio
        case .picture: .picture
        case .keypad: .keypad
        }
    }
}

/// Windows' `--dock-bg`: what the banner and the seek bubble sit on, a dark grey that
/// still reads over a black picture.
private let dockBackground = Color(white: 0.13).opacity(0.94)

/// "21:04", in the viewer's own clock style.
private func wallClock(_ unix: Int64) -> String {
    clockTime(unix: unix)
}

#if os(iOS)
/// `Scrub.tsx`'s `SeekBubble`: a 160×90 picture when there is one, the time, and what
/// the moment was.
private struct SeekBubble: View {
    let time: String
    let lines: [String?]
    let image: CGImage?

    static let width: CGFloat = 176

    var body: some View {
        VStack(spacing: 6) {
            if let image {
                Image(decorative: image, scale: 1)
                    .resizable()
                    .scaledToFill()
                    .frame(width: 160, height: 90)
                    .background(.black)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
            }
            Text(time).font(.subheadline.weight(.heavy)).monospacedDigit()
            ForEach(Array(lines.compactMap { $0?.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }.enumerated()), id: \.offset) { _, line in
                Text(line).font(.caption.bold()).foregroundStyle(.white.opacity(0.7)).lineLimit(1)
            }
        }
        .frame(minWidth: 64, maxWidth: image == nil ? 176 : 160)
        .padding(image == nil ? EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)
                              : EdgeInsets(top: 8, leading: 8, bottom: 8, trailing: 8))
        .foregroundStyle(.white)
        .background(dockBackground, in: RoundedRectangle(cornerRadius: 16))
        .accessibilityHidden(true)
    }
}
#endif

/// Windows' `ChannelBanner` over the player: the channel's logo, number and name, what is
/// on now with its times and how far through it is, what is on next, and the time.
///
/// On an iPhone held upright the time is left to the status bar above it, and the logo and
/// number are a size smaller: Windows never lays the banner out that narrow, and there it
/// left the name a letter or two.
private struct ChannelBannerView: View {
    let banner: PlayerSession.ChannelBanner
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    private var narrow: Bool { sizeClass == .compact }
    #else
    private let narrow = false
    #endif

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            content(now: context.date)
        }
    }

    private func content(now: Date) -> some View {
        HStack(spacing: 16) {
            ChannelTile(name: banner.channel.name, number: banner.channel.number, logo: banner.channel.logo,
                        size: narrow ? 44 : logoSize)
            if let number = banner.channel.number {
                Text(String(number))
                    .font(.system(narrow ? .title2 : numberStyle, design: .rounded).weight(.heavy))
                    .monospacedDigit()
                    .foregroundStyle(.tint)
                    .lineLimit(1).fixedSize()
            }
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(banner.channel.name).font(.headline.weight(.heavy)).lineLimit(1)
                    // Windows' quiet tag: HD, FHD, 4K.
                    if let quality = banner.channel.quality {
                        Text(quality)
                            .font(.caption2.weight(.heavy))
                            .padding(.horizontal, 10).frame(height: 20)
                            .background(.white.opacity(0.16), in: Capsule())
                            .fixedSize()
                    }
                }
                if let on = banner.guide?.now {
                    Text(on.title).font(.subheadline.weight(.semibold)).lineLimit(1)
                    HStack(spacing: 12) {
                        Text("\(wallClock(on.start))–\(wallClock(on.stop))").monospacedDigit()
                        ProgressView(value: through(on, now: now))
                            .tint(.white)
                            .frame(maxWidth: 280)
                            .accessibilityLabel("Programme progress")
                    }
                    .font(.caption.bold())
                    .foregroundStyle(.white.opacity(0.7))
                    if let next = banner.guide?.next {
                        Text("Next: \(wallClock(next.start)) \(next.title)")
                            .font(.caption.bold()).foregroundStyle(.white.opacity(0.7)).lineLimit(1)
                    }
                } else if banner.guide != nil {
                    Text("No guide data for this channel")
                        .font(.caption.bold()).foregroundStyle(.white.opacity(0.7)).lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if !narrow {
                // `clockTime`, not the locale: the player is outside the theme's environment.
                Text(clockTime(now))
                    .font(.title2.weight(.heavy))
                    .monospacedDigit()
                    .lineLimit(1).fixedSize()
            }
        }
        .foregroundStyle(.white)
        .padding(.vertical, 14).padding(.leading, 14).padding(.trailing, 22)
        .frame(maxWidth: maxWidth)
        .background(dockBackground, in: RoundedRectangle(cornerRadius: 26))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(spoken)
    }

    /// How far through the programme now is, 0…1.
    private func through(_ on: Programme, now: Date) -> Double {
        let span = Double(max(1, on.stop - on.start))
        return min(1, max(0, (now.timeIntervalSince1970 - Double(on.start)) / span))
    }

    private var spoken: String {
        let number = banner.channel.number.map { "\($0) " } ?? ""
        let now = banner.guide?.now.map { ", now \($0.title)" } ?? ""
        return number + banner.channel.name + now
    }

    #if os(tvOS)
    private let logoSize: CGFloat = 80
    private let maxWidth: CGFloat = 760
    private let numberStyle: Font.TextStyle = .largeTitle
    #else
    private let logoSize: CGFloat = 52
    private let maxWidth: CGFloat = 560
    /// Windows' `--fs-2xl`: a five-digit number still leaves the name its room on a phone.
    private let numberStyle: Font.TextStyle = .title
    #endif
}
