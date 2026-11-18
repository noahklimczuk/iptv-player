import SwiftUI

/// Where the remote's focus can be in the player (tvOS).
enum PlayerFocus: Hashable {
    case backdrop, back, playPause, forward, subtitles, audio, picture, close
    case skip, upNextPlay, sleepKeep
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
                } else if session.panel == nil, let until = session.seekHintUntil, until > .now {
                    // The remote jumped with the controls hidden: just where it landed.
                    VStack {
                        Spacer()
                        ProgressRow(session: session, interactive: false)
                    }
                    .padding(edge)
                    .allowsHitTesting(false)
                }
                status
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
        // Keep the controls up while the remote moves through them.
        .onChange(of: focus.wrappedValue) { _, new in
            if let new, new != .backdrop { session.poke() }
        }
        .onChange(of: session.controlsVisible) { _, visible in
            if visible, session.activeMarker == nil, !session.upNextVisible {
                focus.wrappedValue = .playPause
            } else if !visible {
                focus.wrappedValue = .backdrop
            }
        }
        // Skip and Up Next take the remote's focus as they appear, as a TV's do.
        .onChange(of: session.activeMarker?.startSecs) { _, start in
            if start != nil, session.panel == nil { focus.wrappedValue = .skip }
        }
        .onChange(of: session.upNextVisible) { _, visible in
            if visible, session.panel == nil { focus.wrappedValue = .upNextPlay }
        }
        .onChange(of: session.sleep.warning) { _, warning in
            if warning { focus.wrappedValue = .sleepKeep }
        }
        #endif
    }

    /// Opening, or sound without a picture.
    @ViewBuilder private var status: some View {
        VStack {
            HStack {
                switch session.status {
                case .opening(let source, let total) where !session.controlsVisible:
                    Label(total > 1
                          ? "Opening \(session.item.title) (source \(source) of \(total))"
                          : "Opening \(session.item.title)",
                          systemImage: "dot.radiowaves.left.and.right")
                        .padding()
                        .background(.ultraThinMaterial, in: Capsule())
                case .noPicture(let message) where !session.controlsVisible:
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
            Button("Close", action: close)
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
                #if os(iOS)
                Button("Close", systemImage: "xmark", action: close)
                    .labelStyle(.iconOnly)
                    .font(.title3)
                    .padding(8)
                    .background(.ultraThinMaterial, in: Circle())
                #endif
            }
            Spacer()
            ProgressRow(session: session, interactive: true)
            HStack(spacing: spacing) {
                if session.seekable {
                    transport("Back 10 seconds", "gobackward.10", .back) { session.seek(by: -10) }
                }
                transport(session.state.paused ? "Play" : "Pause",
                          session.state.paused ? "play.fill" : "pause.fill", .playPause) { session.togglePause() }
                if session.seekable {
                    transport("Forward 30 seconds", "goforward.30", .forward) { session.seek(by: 30) }
                }
                Spacer()
                menuButton("Subtitles", value: subtitleChoice, icon: "captions.bubble", tab: .subtitles)
                if session.state.audioTracks.count > 1 {
                    menuButton("Audio", value: audioChoice, icon: "waveform", tab: .audio)
                }
                menuButton("Picture", value: nil, icon: "slider.horizontal.3", tab: .picture)
            }
            .labelStyle(.iconOnly)
        }
        .padding(edge)
        .foregroundStyle(.white)
        .background {
            LinearGradient(colors: [.black.opacity(0.7), .clear, .clear, .black.opacity(0.8)],
                           startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea()
                .allowsHitTesting(false)
        }
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
        .focused(focus, equals: tab == .subtitles ? .subtitles : tab == .audio ? .audio : .picture)
        #if os(iOS)
        .buttonStyle(.plain)
        .padding(6)
        #endif
    }
}

/// The progress bar and its times, or "Live".
private struct ProgressRow: View {
    @Bindable var session: PlayerSession
    let interactive: Bool
    /// While a finger drags the bar: where it is, so the bar doesn't jump back.
    @State private var dragging: Double?

    var body: some View {
        if session.isLive {
            Label("Live", systemImage: "dot.radiowaves.left.and.right")
                .font(.caption.bold())
                .padding(.horizontal, 10).padding(.vertical, 4)
                .background(.red, in: Capsule())
                .foregroundStyle(.white)
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

    @ViewBuilder private var bar: some View {
        #if os(iOS)
        if interactive {
            Slider(
                value: Binding(get: { dragging ?? session.state.position }, set: { dragging = $0 }),
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
                    Picker("Menu", selection: Binding(get: { tab }, set: { session.panel = $0 })) {
                        Text("Subtitles").tag(PlayerSession.Panel.subtitles)
                        Text("Audio").tag(PlayerSession.Panel.audio)
                        Text("Picture").tag(PlayerSession.Panel.picture)
                    }
                    .pickerStyle(.segmented)
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
        case .catchup: "End of programme"
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

/// Wraps chips onto as many lines as they need.
private struct FlowLayout: Layout {
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
                subviews[index].place(at: CGPoint(x: x, y: bounds.minY + row.y), proposal: ProposedViewSize(size))
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
