import SwiftUI
#if os(iOS)
import AVKit
#endif

/// Where the remote's focus can be in the player (tvOS).
enum PlayerFocus: Hashable {
    case backdrop, back, playPause, forward, subtitles, audio, picture, close
    case channelDown, channelUp, lastChannel, keypad, backToLive
    case stats, record, mute, nextEpisode, guide
    case skip, upNextPlay, upNextCancel, sleepKeep

    /// The top bar's and the dock's buttons: focus on one of these is the viewer using
    /// the controls, which keeps them up. Skip, Up Next and the sleep warning stand alone.
    var isChrome: Bool {
        switch self {
        case .backdrop, .skip, .upNextPlay, .upNextCancel, .sleepKeep: false
        default: true
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

// MARK: Windows' player look

/// Windows' player tokens (`glass.css`; the contrast theme's own from `tokens.css`): the
/// dark glass the controls sit on over any picture, whatever the app's theme.
struct DockTokens {
    let contrast: Bool

    init(_ theme: AuroraTheme) { contrast = theme == .contrast }

    static let yellow = Color(red: 1, green: 0xDD / 255, blue: 0)
    /// `--live`.
    static let live = Color(red: 1, green: 0x5A / 255, blue: 0x36 / 255)
    private static let smoke = Color(red: 18 / 255, green: 18 / 255, blue: 22 / 255)

    /// `--dock-tint`: thick enough to read white type over a white frame. Windows can't
    /// blur the picture behind it (mpv draws beneath the page), so neither does this.
    var tint: Color { contrast ? .black : Self.smoke.opacity(0.62) }
    let text = Color.white
    var muted: Color { contrast ? .white : .white.opacity(0.74) }
    /// `--dock-btn` and `--dock-btn-hover`.
    var button: Color { contrast ? .black : .white.opacity(0.12) }
    var buttonHover: Color { contrast ? Color(white: 0x1F / 255) : .white.opacity(0.2) }
    /// `--dock-ink`: the play button, and a button that is on.
    var ink: Color { contrast ? Self.yellow : .white }
    var inkText: Color { contrast ? .black : Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255) }
    /// `--dock-accent`: the accent, 45% of it in white, to read on the glass.
    var accent: Color { contrast ? Self.yellow : Color(red: 160 / 255, green: 174 / 255, blue: 1) }
    var track: Color { contrast ? Color(white: 0x55 / 255) : .white.opacity(0.22) }
    /// `--player-chip`: the lighter glass of the tags, the small buttons and the stats.
    var chip: Color { contrast ? .black : Self.smoke.opacity(0.45) }
    /// `--glass-video-rim`, as a hairline.
    var rim: Color { contrast ? .white : .white.opacity(0.2) }
    var rimWidth: CGFloat { contrast ? 1 : 0.5 }
}

extension View {
    /// `--dock-bg` (the tint, its top lit a little) with its rim and, for the dock and the
    /// cards, `--shadow-dock`.
    func dockGlass<S: InsettableShape>(_ shape: S, _ dock: DockTokens, fill: Color? = nil,
                                       shadow: Bool = true) -> some View {
        background {
            ZStack {
                shape.fill(fill ?? dock.tint)
                if fill == nil, !dock.contrast {
                    shape.fill(LinearGradient(stops: [.init(color: .white.opacity(0.1), location: 0),
                                                      .init(color: .clear, location: 0.55)],
                                              startPoint: .top, endPoint: .bottom))
                }
                shape.strokeBorder(dock.rim, lineWidth: dock.contrast && shadow ? 2 : dock.rimWidth)
            }
            .compositingGroup()
            .shadow(color: .black.opacity(shadow && !dock.contrast ? 0.38 : 0), radius: 24, y: 18)
        }
    }
}

/// The player's sizes: Windows' pixels where there is Windows' room, a little smaller on
/// a phone, and larger across a room (Windows' own large-text sizes are about a third
/// up).
struct DockMetrics {
    /// The width the controls have.
    let width: CGFloat
    let height: CGFloat
    /// The dock's own height as last laid out, or 0 before it has been.
    var measuredDock: CGFloat = 0

    /// Windows' narrow window (under 760 px): the dock's timeline on a row of its own,
    /// and no programme cards.
    var narrow: Bool {
        #if os(tvOS)
        false
        #else
        width < 760
        #endif
    }

    var scale: CGFloat {
        #if os(tvOS)
        1.33
        #else
        narrow ? 0.92 : 1
        #endif
    }

    func px(_ value: CGFloat) -> CGFloat { value * scale }

    /// Windows' type: `--fs-xs` 12, `--fs-sm` 13, `--fs-md` 15, `--fs-lg` 17, `--fs-2xl` 26.
    func font(_ size: CGFloat, _ weight: Font.Weight = .heavy) -> Font {
        .system(size: size * scale, weight: weight)
    }

    /// Windows' 28 px from the window's edge.
    var edge: CGFloat {
        #if os(tvOS)
        28
        #else
        narrow ? 12 : 24
        #endif
    }

    /// How much of the bottom the dock takes, for what sits above it: as laid out, or
    /// one row of buttons and the timeline before it has been.
    var dockHeight: CGFloat {
        if measuredDock > 0 { return measuredDock }
        return narrow ? px(64 + 24 + 52) + 14 : px(64 + 24)
    }
}

/// Windows' `.pl-btn` family: square, the dock's own grey, white ink when it is on (and,
/// on a TV, when it has the remote's focus).
private struct DockButtonStyle: ButtonStyle {
    enum Kind { case plain, primary, glass, glassLarge, small }
    let kind: Kind
    var active = false
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let dock = DockTokens(theme)
        let shape = RoundedRectangle(cornerRadius: m.px(radius), style: .continuous)
        let inked = kind == .primary || active || focused
        return configuration.label
            .frame(width: m.px(side), height: m.px(side))
            .foregroundStyle(inked ? dock.inkText : dock.text)
            .background {
                shape.fill(inked ? dock.ink : glass ? dock.chip : configuration.isPressed ? dock.buttonHover : dock.button)
                if glass, !inked { shape.strokeBorder(dock.rim, lineWidth: dock.rimWidth) }
            }
            .contentShape(shape)
            // The play button and lit toggles are ink already, so focus also glows: the
            // remote needs a cue on every button, not just the glass ones.
            .shadow(color: (theme == .contrast ? dock.accent : Color.white).opacity(focused ? 0.55 : 0),
                    radius: m.px(14))
            .scaleEffect(focused ? 1.12 : configuration.isPressed ? 0.94 : 1)
            .animation(.easeOut(duration: 0.15), value: focused)
    }

    private var glass: Bool { kind == .glass || kind == .glassLarge }

    private var side: CGFloat {
        switch kind {
        case .plain, .glassLarge: 48
        case .primary: 64
        case .glass: 34
        case .small: 36
        }
    }

    private var radius: CGFloat {
        switch kind {
        case .plain: 16
        case .primary: 22
        case .glass, .glassLarge: 14
        case .small: 12
        }
    }
}

#if os(iOS)
/// The system's AirPlay picker, drawn as one of the top bar's glass buttons.
private struct AirPlayButton: View {
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let dock = DockTokens(theme)
        let shape = RoundedRectangle(cornerRadius: m.px(14), style: .continuous)
        RoutePicker(tint: UIColor(dock.text), active: UIColor(dock.accent))
            .frame(width: m.px(34), height: m.px(34))
            .background {
                shape.fill(dock.chip)
                shape.strokeBorder(dock.rim, lineWidth: dock.rimWidth)
            }
            .accessibilityLabel("AirPlay")
    }
}

private struct RoutePicker: UIViewRepresentable {
    let tint: UIColor
    let active: UIColor

    func makeUIView(context: Context) -> AVRoutePickerView {
        let picker = AVRoutePickerView()
        picker.prioritizesVideoDevices = true
        return picker
    }

    func updateUIView(_ picker: AVRoutePickerView, context: Context) {
        picker.tintColor = tint
        picker.activeTintColor = active
    }
}
#endif

/// One of the dock's or the top bar's buttons: an icon, named for VoiceOver and the
/// pointer.
private struct DockButton: View {
    let label: String
    let icon: String
    var kind: DockButtonStyle.Kind = .plain
    var active = false
    let id: PlayerFocus?
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: m.px(iconSize), weight: kind == .primary ? .bold : .semibold))
        }
        .buttonStyle(DockButtonStyle(kind: kind, active: active, m: m))
        .focused(focus, equals: id ?? .backdrop)
        .accessibilityLabel(label)
        .accessibilityAddTraits(active ? .isSelected : [])
        #if os(iOS)
        .help(label)
        #endif
    }

    private var iconSize: CGFloat {
        switch kind {
        case .primary: 28
        case .plain, .glassLarge: 21
        case .glass, .small: 15
        }
    }
}

/// A button with words on it (Back to live, Record, Play now): Windows draws each in its
/// own colours, and a TV's focus inks it.
private struct PillStyle: ButtonStyle {
    var foreground: Color
    var background: Color
    var height: CGFloat
    var radius: CGFloat
    var font: Font
    var rim: Color?
    var padding: CGFloat = 14
    var rimWidth: CGFloat?
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let dock = DockTokens(theme)
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        return configuration.label
            .font(font)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, padding)
            .frame(height: height)
            .foregroundStyle(focused ? dock.inkText : foreground)
            .background {
                shape.fill(focused ? dock.ink : background)
                if let rim, !focused { shape.strokeBorder(rim, lineWidth: rimWidth ?? dock.rimWidth) }
            }
            .contentShape(shape)
            .opacity(configuration.isPressed ? 0.8 : 1)
            .scaleEffect(focused ? 1.08 : 1)
            .animation(.easeOut(duration: 0.15), value: focused)
    }
}

/// Windows' `.pl-dot`: the live red dot.
private struct LiveDot: View {
    let m: DockMetrics
    var body: some View {
        Circle().fill(DockTokens.live).frame(width: m.px(8), height: m.px(8)).accessibilityHidden(true)
    }
}

// MARK: Overlay

/// Everything over the picture: the Apple counterpart of `src-ui` `PlayerOverlay`.
struct PlayerOverlay: View {
    @Bindable var session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let close: () -> Void
    let retry: () -> Void
    /// The player is its own presentation: it reads the theme for itself.
    @AppStorage(AppearanceKey.theme) private var theme: AuroraTheme = .dark
    /// How tall the dock is, for the sheet, Skip and Up Next that sit above it.
    @State private var dockHeight: CGFloat = 0

    var body: some View {
        GeometryReader { geo in
            let m = DockMetrics(width: geo.size.width, height: geo.size.height, measuredDock: dockHeight)
            ZStack {
                if session.controlsVisible {
                    scrim(height: geo.size.height)
                }
                if case .failed(let message) = session.status {
                    ErrorPanel(session: session, message: message, m: m, retry: retry, close: close)
                } else {
                    waiting(m)
                    if session.controlsVisible && chromeAllowed {
                        PlayerChrome(session: session, focus: focus, m: m, dockHeight: $dockHeight, close: close)
                            .transition(.opacity)
                    } else if session.panel == nil, session.seekHintUntil != nil {
                        // The remote jumped with the controls hidden: just where it landed.
                        VStack {
                            Spacer()
                            ProgressRow(session: session, interactive: false, m: m)
                                .padding(m.px(14))
                                .frame(maxWidth: m.px(1100))
                                .dockGlass(RoundedRectangle(cornerRadius: m.px(30), style: .continuous),
                                           DockTokens(theme))
                        }
                        .padding(m.edge)
                        .allowsHitTesting(false)
                    }
                    zapping(m)
                    corner(m)
                    if let panel = session.panel {
                        WatchPanel(session: session, tab: panel, focus: focus, m: m)
                            .transition(.move(edge: .trailing).combined(with: .opacity))
                    }
                    if session.sleep.warning {
                        SleepWarning(session: session, focus: focus, m: m)
                    }
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
        }
        // Over a picture, whatever the rest of the app is set to: a light menu on a film
        // is a lamp switched on in the dark. The theme still picks the accent and, in
        // high contrast, the controls' own black and yellow.
        .environment(\.auroraTheme, theme)
        .environment(\.colorScheme, .dark)
        .animation(.easeInOut(duration: 0.2), value: session.controlsVisible)
        .animation(.easeInOut(duration: 0.2), value: session.panel)
        .animation(.easeInOut(duration: 0.18), value: session.statsOpen)
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
            DispatchQueue.main.async { focus.wrappedValue = old.opener }
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

    /// On a TV the open panel has the remote to itself: the dock under it would take
    /// focus moving out of the panel's list.
    private var chromeAllowed: Bool {
        #if os(tvOS)
        session.panel == nil
        #else
        true
        #endif
    }

    #if os(tvOS)
    /// Where focus goes when what held it disappears.
    private func restFocus() {
        focus.wrappedValue = session.controlsVisible ? .playPause : .backdrop
    }
    #endif

    /// Windows' `.pl-scrim`: the top 28% and the bottom 42% darkened, for the bars.
    private func scrim(height: CGFloat) -> some View {
        let ink = Color(red: 10 / 255, green: 12 / 255, blue: 20 / 255)
        return VStack(spacing: 0) {
            LinearGradient(colors: [ink.opacity(0.55), .clear], startPoint: .top, endPoint: .bottom)
                .frame(height: height * 0.28)
            Spacer(minLength: 0)
            LinearGradient(colors: [.clear, ink.opacity(0.7)], startPoint: .top, endPoint: .bottom)
                .frame(height: height * 0.42)
        }
        .ignoresSafeArea()
        .allowsHitTesting(false)
        .transition(.opacity)
    }

    /// Windows' `Waiting`: a spinner in the middle while the stream opens, with what is
    /// opening; or the sound-without-picture notice in its place.
    @ViewBuilder private func waiting(_ m: DockMetrics) -> some View {
        switch session.status {
        case .opening(let source, let total):
            VStack(spacing: m.px(10)) {
                Spinner(m: m)
                Text(session.shownTitle)
                    .font(m.font(26))
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
                Text(source > 1 && total > 1 ? "Trying source \(source) of \(total)…" : "Opening…")
                    .font(m.font(13, .bold))
                    .opacity(0.85)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, m.edge)
            .allowsHitTesting(false)
            .accessibilityElement(children: .combine)
        case .noPicture(let message) where session.panel == nil:
            Label(message, systemImage: "speaker.wave.2")
                .font(m.font(13, .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, m.px(14)).padding(.vertical, m.px(10))
                .dockGlass(Capsule(), DockTokens(theme), fill: DockTokens(theme).chip, shadow: false)
                .padding(.horizontal, m.edge)
                .allowsHitTesting(false)
        default:
            EmptyView()
        }
    }

    /// The number being typed, top right; the channel just tuned, top centre, clear of
    /// the controls (Windows' `ChannelBanner` over the player); and anything said about a
    /// zap, bottom left. None of them raises the controls.
    private func zapping(_ m: DockMetrics) -> some View {
        VStack {
            HStack(alignment: .top) {
                Spacer()
                if let banner = session.channelBanner, session.panel == nil {
                    ChannelBannerView(banner: banner)
                        // Below the top bar while the controls are up, clear of it.
                        .padding(.top, session.controlsVisible ? m.px(76) : 0)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
                Spacer()
            }
            .overlay(alignment: .topTrailing) {
                if !session.digits.isEmpty {
                    Text(session.digits)
                        .font(.system(size: digitSize, weight: .bold, design: .rounded))
                        .monospacedDigit()
                        .foregroundStyle(.white)
                        .padding(.horizontal, 20).padding(.vertical, 8)
                        .dockGlass(RoundedRectangle(cornerRadius: 16, style: .continuous), DockTokens(theme))
                        .accessibilityLabel("Channel \(session.digits)")
                }
            }
            Spacer()
            HStack {
                if let banner = session.banner, session.panel == nil {
                    Text(banner)
                        .font(m.font(15))
                        .foregroundStyle(.white)
                        .lineLimit(2)
                        .padding(.horizontal, m.px(18)).padding(.vertical, m.px(10))
                        .dockGlass(Capsule(), DockTokens(theme))
                        .transition(.opacity)
                }
                Spacer()
            }
            .padding(.bottom, m.dockHeight + m.px(16))
        }
        .padding(m.edge)
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

    /// Skip and Up Next, bottom right, one stack above where the dock sits (`App.tsx`).
    /// Both can be live during the credits, so they queue rather than overlap. The stack
    /// stays put when the controls fade: one that dropped into their place would take a
    /// tap meant for a control that had just gone.
    private func corner(_ m: DockMetrics) -> some View {
        VStack(alignment: .trailing, spacing: m.px(12)) {
            Spacer()
            if session.panel == nil {
                if session.upNextVisible, let next = session.aids?.nextEpisode {
                    UpNextCard(session: session, episode: next, focus: focus, m: m)
                }
                if let marker = session.activeMarker {
                    SkipButton(session: session, marker: marker, focus: focus, m: m)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
        .padding(m.edge)
        .padding(.bottom, m.dockHeight + m.px(16))
    }
}

/// Windows' `.pl-spinner`: a white ring turning.
private struct Spinner: View {
    let m: DockMetrics
    @State private var turned = false

    var body: some View {
        Circle()
            .trim(from: 0, to: 0.25)
            .stroke(.white, style: StrokeStyle(lineWidth: m.px(4), lineCap: .butt))
            .background { Circle().stroke(.white.opacity(0.25), lineWidth: m.px(4)) }
            .frame(width: m.px(48), height: m.px(48))
            .rotationEffect(.degrees(turned ? 360 : 0))
            .animation(.linear(duration: 0.9).repeatForever(autoreverses: false), value: turned)
            .onAppear { turned = true }
            .accessibilityHidden(true)
    }
}

/// Windows' `ErrorPanel`: a card in the middle, in the app's own colours, with what went
/// wrong and only the actions that work.
private struct ErrorPanel: View {
    let session: PlayerSession
    let message: String
    let m: DockMetrics
    let retry: () -> Void
    let close: () -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack(spacing: 0) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: m.px(30), weight: .semibold))
                .foregroundStyle(theme == .contrast ? Color.white : Color.red)
                .padding(.bottom, m.px(12))
            Text("Could not play \(session.shownTitle)")
                .font(m.font(17, .bold))
                .multilineTextAlignment(.center)
                .padding(.bottom, m.px(8))
            Text(message)
                .font(m.font(13, .regular))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.bottom, m.px(16))
            FlowLayout(spacing: m.px(8)) {
                action("Retry", retry)
                // A dead channel isn't the end of watching: the next one is a press away.
                if session.isLive {
                    action("Channel down") { session.step(forward: false) }
                    action("Channel up") { session.step(forward: true) }
                }
                action("Close", close)
            }
            .fixedSize()
        }
        .padding(m.px(32))
        .frame(width: min(m.px(460), m.width - 48))
        .background(theme.surface, in: RoundedRectangle(cornerRadius: m.px(18), style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: m.px(18), style: .continuous)
                .strokeBorder(theme == .contrast ? Color.white : theme.borderStrong, lineWidth: theme == .contrast ? 2 : 1)
        }
        .shadow(color: .black.opacity(0.3), radius: 20, y: 12)
        // In the app's own colours, as Windows' `--bg-elevated` card is.
        .environment(\.colorScheme, theme.scheme)
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(.isModal)
    }

    private func action(_ title: String, _ run: @escaping () -> Void) -> some View {
        Button(title, action: run)
            .buttonStyle(PillStyle(foreground: .primary, background: theme.background,
                                   height: m.px(36), radius: m.px(14), font: m.font(14, .semibold),
                                   rim: theme == .contrast ? .white : theme.borderStrong, padding: m.px(16), rimWidth: 1))
    }
}

// MARK: Controls

/// The top bar, the programme cards and the dock (`PlayerOverlay.tsx`'s `Chrome`).
private struct PlayerChrome: View {
    @Environment(AppModel.self) private var model
    @Bindable var session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    @Binding var dockHeight: CGFloat
    let close: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            TopBar(session: session, focus: focus, m: m, close: close)
            if session.statsOpen {
                StatsPanel(stats: session.stats, m: m)
                    .frame(maxWidth: .infinity, alignment: .trailing)
                    .padding(.top, m.px(12))
                    .transition(.opacity)
            }
            Spacer(minLength: m.px(12))
            if session.isLive, !m.narrow, let guide = session.liveGuide {
                LiveCards(guide: guide, m: m) { session.recordNext(model: model) }
                    .padding(.bottom, m.px(16))
            }
            Dock(session: session, focus: focus, m: m) {
                model.openGuide()
                close()
            }
            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { dockHeight = $0 }
        }
        .padding(.horizontal, m.edge)
        .padding(.top, m.px(12))
        .padding(.bottom, m.edge)
        .foregroundStyle(.white)
        .task(id: session.item) { await session.loadLiveGuide() }
        // What the top bar's resolution tag and the stats say, kept fresh while the
        // controls are up (Windows' engine sends them each second).
        .task {
            while !Task.isCancelled {
                session.refreshStats()
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }
}

/// Windows' `TopBar`: Back, the title pill with its Live badge, then the tags, Stats and
/// (live) Record.
private struct TopBar: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    @Bindable var session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    let close: () -> Void

    var body: some View {
        HStack(spacing: m.px(12)) {
            DockButton(label: "Back", icon: "chevron.left", kind: .glassLarge, id: .close, focus: focus, m: m,
                       action: close)
            titlePill
                .frame(maxWidth: m.width * (m.narrow ? 0.6 : 0.52), alignment: .leading)
            Spacer(minLength: 0)
            HStack(spacing: m.px(6)) {
                // A phone upright has no room for them beside the title; the menus say
                // the same.
                if !m.narrow {
                    ForEach(tags, id: \.self) { tag in
                        Text(tag)
                            .font(m.font(12))
                            .lineLimit(1)
                            .padding(.horizontal, m.px(10))
                            .frame(height: m.px(26))
                            .background(DockTokens(theme).chip, in: Capsule())
                            .fixedSize()
                    }
                }
                #if os(iOS)
                // AirPlay: the sound to a speaker or a TV (the picture goes with the
                // screen when it is mirrored). An Apple control Windows has no need of.
                AirPlayButton(m: m)
                #endif
                DockButton(label: "Playback stats", icon: "info", kind: .glass, active: session.statsOpen,
                           id: .stats, focus: focus, m: m) {
                    session.statsOpen.toggle()
                    session.poke()
                }
                // Live only: a film is already a file on the provider's disk.
                if session.isLive {
                    DockButton(label: "Record what's on", icon: "record.circle", kind: .glass, id: .record,
                               focus: focus, m: m) { session.recordNow(model: model) }
                }
            }
        }
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private var titlePill: some View {
        let dock = DockTokens(theme)
        let name = session.shownTitle
        return HStack(spacing: m.px(12)) {
            Text(name.prefix(1).uppercased())
                .font(.system(size: m.px(18), weight: .heavy))
                .foregroundStyle(.white)
                .frame(width: m.px(44), height: m.px(44))
                .background(theme == .contrast ? Color(white: 0x1F / 255) : ChannelTile.hue(name),
                            in: RoundedRectangle(cornerRadius: m.px(14), style: .continuous))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 0) {
                Text(name).font(m.font(17)).lineLimit(1)
                if let subtitle = session.heading?.subtitle {
                    Text(subtitle).font(m.font(13, .bold)).foregroundStyle(dock.muted).lineLimit(1)
                }
            }
            .layoutPriority(1)
            if session.isLive { badge }
        }
        .padding(.vertical, m.px(8)).padding(.leading, m.px(8)).padding(.trailing, m.px(18))
        .dockGlass(RoundedRectangle(cornerRadius: m.px(20), style: .continuous), dock, shadow: false)
        .accessibilityElement(children: .combine)
    }

    /// "● Live", or how far behind it the viewer is.
    private var badge: some View {
        let behind = session.behindLive
        let live = behind <= 0
        // A phone upright keeps the title's room: the dock's readout says the rest.
        let words = m.narrow ? "↺ \(clock(behind))" : "↺ \(clock(behind)) behind live"
        return Text(live ? "● Live" : words)
            .font(m.font(12))
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, m.px(9)).padding(.vertical, m.px(3))
            .foregroundStyle(live ? Color.white : theme.accentText)
            .background(live ? DockTokens.live : theme.accent, in: Capsule())
            .accessibilityLabel(live ? "Live" : "\(clock(behind)) behind live")
    }

    /// Windows' tags: what the subtitles and audio are set to, and the picture's size
    /// and rate.
    private var tags: [String] {
        let state = session.state
        let subtitles = state.subtitleTracks.first { $0.id == state.subtitleId }?.name ?? "Off"
        let audio = state.audioTracks.first { $0.id == state.audioId }?.name
        var picture: String?
        if let resolution = session.stats?.resolution {
            picture = [resolution, session.stats?.fps.map { "\(Int($0.rounded()))fps" }].compactMap { $0 }
                .joined(separator: " · ")
        }
        var seen = Set<String>()
        return ["CC \(subtitles)", audio, picture].compactMap { $0 }.filter { seen.insert($0).inserted }
    }
}

/// Windows' `StatsPanel` (C18): what the picture is and how the stream is keeping up.
private struct StatsPanel: View {
    let stats: MPVPlayerController.PlaybackStats?
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: m.px(16), verticalSpacing: m.px(3)) {
            ForEach(rows, id: \.0) { key, value in
                GridRow {
                    Text(key).opacity(0.7)
                    Text(value)
                }
            }
        }
        .font(.system(size: m.px(12), design: .monospaced))
        .foregroundStyle(.white)
        .padding(.vertical, m.px(12)).padding(.horizontal, m.px(16))
        .background(DockTokens(theme).chip, in: RoundedRectangle(cornerRadius: m.px(18), style: .continuous))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Playback statistics")
    }

    private var rows: [(String, String)] {
        let s = stats ?? MPVPlayerController.PlaybackStats()
        return [
            ("Resolution", s.resolution ?? "—"),
            ("Video codec", s.videoCodec ?? "—"),
            ("Audio codec", s.audioCodec ?? "—"),
            ("Frame rate", s.fps.flatMap { $0 > 0 ? $0 : nil }.map { String(format: $0.truncatingRemainder(dividingBy: 1) != 0 ? "%.2f fps" : "%.0f fps", $0) } ?? "—"),
            // Windows' `bitrateKbps ? … : '—'`: nothing measured reads as nothing.
            ("Bitrate", s.bitrateKbps.flatMap { $0 > 0 ? $0 : nil }.map { String(format: "%.1f Mbps", Double($0) / 1000) } ?? "—"),
            ("Dropped frames", String(s.droppedFrames)),
            ("Buffer", String(format: "%.1fs", s.bufferSecs)),
            ("Cache", String(format: "%.1f MB", Double(s.cacheBytes) / 1_048_576)),
            ("Hardware decoder", s.hwDecoder ?? "software"),
        ]
    }
}

/// Windows' `Dock`: play, the skips, Back to live, Next episode, the timeline, then the
/// menus, on one bar of glass. Where the window is narrow the timeline takes a row of its
/// own above the buttons.
private struct Dock: View {
    @Bindable var session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    let openGuide: () -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let dock = DockTokens(theme)
        content
            .padding(m.px(12))
            .frame(maxWidth: min(m.px(1100), m.width))
            .dockGlass(RoundedRectangle(cornerRadius: m.px(m.narrow ? 24 : 30), style: .continuous), dock)
            #if os(tvOS)
            .focusSection()
            #endif
    }

    @ViewBuilder private var content: some View {
        #if os(tvOS)
        wide
        #else
        if m.narrow {
            narrow
        } else {
            // A live channel's own buttons can outgrow an iPad held upright: then it is
            // laid out as a narrow window's.
            ViewThatFits(in: .horizontal) {
                wide
                narrow
            }
        }
        #endif
    }

    private var wide: some View {
        HStack(spacing: m.px(10)) {
            leading
            ProgressRow(session: session, interactive: true, m: m)
                .padding(.horizontal, m.px(8))
                .frame(minWidth: m.px(120), idealWidth: m.px(120), maxWidth: .infinity)
            trailing
        }
    }

    private var narrow: some View {
        VStack(alignment: .leading, spacing: m.px(10)) {
            ProgressRow(session: session, interactive: true, m: m)
                .padding(.horizontal, m.px(8))
            FlowLayout(spacing: m.px(10)) {
                leading
                trailing
            }
        }
    }

    @ViewBuilder private var leading: some View {
        DockButton(label: session.state.paused ? "Play" : "Pause",
                   icon: session.state.paused ? "play.fill" : "pause.fill",
                   kind: .primary, id: .playPause, focus: focus, m: m) { session.togglePause() }
        if session.seekable {
            DockButton(label: "Back 10 seconds", icon: "gobackward.10", id: .back, focus: focus, m: m) {
                session.seek(by: -10)
            }
            DockButton(label: "Forward 10 seconds", icon: "goforward.10", id: .forward, focus: focus, m: m) {
                session.seek(by: 10)
            }
        }
        if session.isLive, session.behindLive > 0 {
            // Windows' Back to live (T): only while behind it.
            Button { session.backToLive() } label: {
                HStack(spacing: m.px(6)) {
                    LiveDot(m: m)
                    Text("Back to live")
                }
            }
            .buttonStyle(PillStyle(foreground: DockTokens.live, background: DockTokens(theme).button,
                                   height: m.px(48), radius: m.px(16), font: m.font(13), rim: nil))
            .focused(focus, equals: .backToLive)
        }
        if let next = session.aids?.nextEpisode {
            DockButton(label: "Next episode: S\(next.season) E\(next.episode)", icon: "forward.end.fill",
                       id: .nextEpisode, focus: focus, m: m) { session.playNext() }
        }
    }

    @ViewBuilder private var trailing: some View {
        if session.isLive {
            // Not on Windows' dock, where Page Up and Page Down and the number keys do
            // it: a phone or a TV remote has no keys for them.
            DockButton(label: "Channel down", icon: "chevron.down", id: .channelDown, focus: focus, m: m) {
                session.step(forward: false)
            }
            DockButton(label: "Channel up", icon: "chevron.up", id: .channelUp, focus: focus, m: m) {
                session.step(forward: true)
            }
            if let last = session.lastChannel {
                // Windows' Backspace: the channel before this one.
                DockButton(label: "Back to \(last.name)", icon: "arrow.uturn.backward", id: .lastChannel,
                           focus: focus, m: m) { session.backToLast() }
            }
            menu("Channel number", icon: "number", tab: .keypad)
        }
        DockButton(label: session.muted ? "Unmute" : "Mute",
                   icon: session.muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
                   id: .mute, focus: focus, m: m) { session.toggleMute() }
        if session.isLive {
            DockButton(label: "TV Guide", icon: "square.grid.2x2", id: .guide, focus: focus, m: m,
                       action: openGuide)
        }
        menu("Subtitles", icon: "captions.bubble", tab: .subtitles)
        menu("Audio track", icon: "waveform", tab: .audio)
        menu("Playback settings", icon: "gearshape", tab: .picture)
    }

    private func menu(_ title: String, icon: String, tab: PlayerSession.Panel) -> some View {
        DockButton(label: title, icon: icon, active: session.panel == tab, id: tab.opener, focus: focus, m: m) {
            session.panel = session.panel == tab ? nil : tab
        }
    }
}

/// Windows' `LiveCards`: on now at the bottom left; next at the bottom right, with a
/// Record button.
private struct LiveCards: View {
    let guide: NowNext
    let m: DockMetrics
    let recordNext: () -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let dock = DockTokens(theme)
        HStack(alignment: .bottom, spacing: m.px(16)) {
            if let on = guide.now {
                VStack(alignment: .leading, spacing: 0) {
                    Text("On now · \(wallClock(on.start))–\(wallClock(on.stop))")
                        .font(m.font(13))
                        .tracking(0.26)
                        .foregroundStyle(dock.accent)
                    Text(on.title)
                        .font(.system(size: m.px(36), weight: .heavy))
                        .lineLimit(1)
                        .padding(.top, m.px(6)).padding(.bottom, m.px(8))
                    if let text = on.description ?? on.subTitle {
                        Text(text)
                            .font(m.font(15, .regular))
                            .foregroundStyle(dock.muted)
                            .lineLimit(2)
                            .lineSpacing(m.px(4))
                    }
                }
                .padding(.vertical, m.px(20)).padding(.horizontal, m.px(22))
                .frame(width: min(m.px(440), m.width * 0.36), alignment: .leading)
                .dockGlass(RoundedRectangle(cornerRadius: m.px(26), style: .continuous), dock)
                .accessibilityElement(children: .combine)
            }
            Spacer(minLength: 0)
            if let next = guide.next {
                HStack(spacing: m.px(12)) {
                    VStack(alignment: .leading, spacing: m.px(2)) {
                        Text("NEXT · \(wallClock(next.start))")
                            .font(m.font(12))
                            .opacity(0.8)
                        Text(next.title).font(m.font(17)).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Button { recordNext() } label: {
                        Label("Record", systemImage: "record.circle")
                    }
                    .buttonStyle(PillStyle(foreground: .white, background: dock.tint, height: m.px(40),
                                           radius: m.px(14), font: m.font(13), rim: dock.rim))
                    .accessibilityLabel("Record \(next.title)")
                }
                .padding(.vertical, m.px(14)).padding(.leading, m.px(18)).padding(.trailing, m.px(14))
                .frame(width: min(m.px(336), m.width * 0.3))
                .background(dock.chip, in: RoundedRectangle(cornerRadius: m.px(22), style: .continuous))
                .accessibilityElement(children: .contain)
            }
        }
        .foregroundStyle(.white)
        #if os(tvOS)
        .focusSection()
        #endif
    }
}

// MARK: Timeline

/// Windows' `Timeline` (`Scrub.tsx`): where playback is and how long it runs, the
/// timeshift buffer, or the programme on air.
private struct ProgressRow: View {
    @Bindable var session: PlayerSession
    let interactive: Bool
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    /// While a finger drags the bar: where it is, so the bar doesn't jump back.
    @State private var dragging: Double?
    /// What the seek preview says about where the finger is.
    @State private var preview: Shown?

    struct Shown {
        let preview: SeekPreview
        let image: CGImage?
    }

    var body: some View {
        Group {
            if session.isLive, let window = session.state.rewind {
                timeshift(window)
            } else if session.isLive {
                onAir
            } else if session.state.duration > 0 {
                VStack(spacing: m.px(6)) {
                    readout {
                        Text(clock(dragging ?? session.state.position))
                        Spacer(minLength: m.px(10))
                        Text(clock(session.state.duration)).foregroundStyle(DockTokens(theme).muted)
                    }
                    bar
                }
            } else {
                // The stream doesn't say how long it is: just where it is.
                VStack(spacing: m.px(6)) {
                    readout {
                        Text(clock(session.state.position))
                        Spacer(minLength: 0)
                    }
                    Track(fraction: 0, height: m.px(10))
                }
            }
        }
        .foregroundStyle(.white)
    }

    private func readout<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 0, content: content)
            .font(m.font(13))
            .monospacedDigit()
            .lineLimit(1)
    }

    /// Live with no buffer: the programme's start, "Live" and the time, its end, and a
    /// bar for how far through it is.
    private var onAir: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let prog = session.liveGuide?.now
            let now = context.date.timeIntervalSince1970
            let through = prog.map { (now - Double($0.start)) / Double(max(1, $0.stop - $0.start)) } ?? 1
            VStack(spacing: m.px(6)) {
                readout {
                    Text(prog.map { wallClock($0.start) } ?? "")
                    Spacer(minLength: m.px(10))
                    HStack(spacing: m.px(5)) {
                        LiveDot(m: m)
                        Text("Live · \(clockTime(context.date))")
                    }
                    .foregroundStyle(DockTokens.live)
                    Spacer(minLength: m.px(10))
                    Text(prog.map { wallClock($0.stop) } ?? "").foregroundStyle(DockTokens(theme).muted)
                }
                Track(fraction: min(1, max(0, through)), height: m.px(10))
                    .accessibilityLabel("Programme progress")
            }
        }
    }

    /// Live with a buffer (`Scrub.tsx`'s timeshift timeline): how far behind live, how
    /// much is held, and a bar from the oldest moment held to live, its live edge marked.
    private func timeshift(_ w: RewindWindow) -> some View {
        // While dragging, the readout says where the finger is, as Windows' preview does.
        let behind = dragging.map { max(0, w.liveSecs - $0) } ?? w.behindSecs
        let atLive = behind <= liveEdgeSecs
        let held = max(0, w.liveSecs - w.startSecs)
        return VStack(spacing: m.px(6)) {
            readout {
                HStack(spacing: m.px(5)) {
                    if atLive { LiveDot(m: m) }
                    Text(atLive ? "Live" : "−" + clock(behind))
                }
                .foregroundStyle(DockTokens.live)
                .accessibilityLabel(atLive ? "Live" : "\(clock(behind)) behind live")
                Spacer(minLength: m.px(10))
                Text(held > 0 ? "\(clock(held)) buffered" : "buffering…")
                    .font(m.font(12))
                    .foregroundStyle(DockTokens(theme).muted)
            }
            timeshiftBar(w)
                .overlay(alignment: .trailing) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(DockTokens.live)
                        .frame(width: m.px(4), height: m.px(16))
                        .offset(x: 1)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
        }
    }

    /// Windows' `LIVE_EDGE_SECS`, which the core's `behind_secs` already uses.
    private let liveEdgeSecs = 5.0

    @ViewBuilder private func timeshiftBar(_ w: RewindWindow) -> some View {
        let upper = max(w.liveSecs, w.startSecs + 1)
        let at = min(max(0, w.positionSecs - w.startSecs), upper - w.startSecs) / (upper - w.startSecs)
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
            .tint(theme.accent)
            .background { Capsule().fill(DockTokens(theme).track).frame(height: 4) }
            // Above the readout row, which sits over this bar.
            .overlay { bubble(in: w.startSecs...upper, lift: 34) { secs, p in
                // `Scrub.tsx`'s timeshift words.
                let back = max(0, w.liveSecs - secs)
                return (back <= liveEdgeSecs ? "Live" : "−" + clock(back),
                        [p?.clockUnix.map(wallClock), p?.programme, p?.label])
            } }
            .accessibilityLabel("Timeshift")
        } else {
            Track(fraction: at, height: m.px(6))
        }
        #else
        Track(fraction: at, height: m.px(6))
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
        let duration = session.state.duration
        #if os(iOS)
        if interactive {
            Slider(
                value: Binding(get: { dragging ?? session.state.position },
                               set: { dragging = $0; session.poke() }),
                in: 0...max(duration, 1),
                onEditingChanged: { editing in
                    if editing {
                        session.poke()
                    } else if let to = dragging {
                        session.seek(to: to)
                        dragging = nil
                    }
                }
            )
            .tint(theme.accent)
            // The unplayed part of the track is drawn dark, which vanishes on the glass.
            .background { Capsule().fill(DockTokens(theme).track).frame(height: 4) }
            .overlay { bubble(in: 0...max(duration, 1), lift: 10) { secs, p in
                (clock(secs), [p?.label])
            } }
            .accessibilityLabel("Seek")
        } else {
            Track(fraction: min(session.state.position, duration) / duration, height: m.px(6))
        }
        #else
        Track(fraction: min(session.state.position, duration) / duration, height: m.px(6))
        #endif
    }
}

/// Windows' `.pl-progress`: the track, and the accent as far as `fraction`.
private struct Track: View {
    let fraction: Double
    let height: CGFloat
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(DockTokens(theme).track)
                Capsule().fill(theme.accent)
                    .frame(width: geo.size.width * min(1, max(0, fraction.isFinite ? fraction : 0)))
            }
        }
        .frame(height: height)
        .accessibilityValue("\(Int((min(1, max(0, fraction.isFinite ? fraction : 0)) * 100).rounded()))%")
    }
}

// MARK: Watch settings

/// Windows' `WatchMenu` (C13): Subtitles, Audio and Picture as tabs of one sheet of glass
/// over the bottom right, above the dock; and, on Apple, the channel keypad.
private struct WatchPanel: View {
    @Bindable var session: PlayerSession
    let tab: PlayerSession.Panel
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    @State private var sleepError: String?
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let dock = DockTokens(theme)
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(tab == .keypad ? "Channel number" : "Watch settings")
                    .font(m.font(26))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                Spacer(minLength: m.px(8))
                DockButton(label: "Close watch settings", icon: "xmark", kind: .small, id: nil, focus: focus,
                           m: m) { session.panel = nil }
            }
            .padding(.bottom, m.px(14))
            if tab != .keypad {
                tabs.padding(.bottom, m.px(14))
            }
            ScrollView {
                VStack(alignment: .leading, spacing: m.px(2)) {
                    switch tab {
                    case .subtitles: subtitles
                    case .audio: audio
                    case .picture: picture
                    case .keypad: Keypad(session: session, m: m)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                #if os(tvOS)
                .padding(.horizontal, 12)
                #endif
            }
            .scrollIndicators(.hidden)
        }
        .foregroundStyle(dock.text)
        .padding(m.px(20))
        .frame(width: min(m.px(420), m.width - 2 * m.edge))
        .frame(maxHeight: max(m.px(240), m.height - m.dockHeight - m.px(120)), alignment: .top)
        .fixedSize(horizontal: false, vertical: true)
        .dockGlass(RoundedRectangle(cornerRadius: m.px(32), style: .continuous), dock)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
        .padding(.horizontal, m.edge)
        .padding(.bottom, m.edge + m.dockHeight + m.px(12))
        #if os(tvOS)
        .focusSection()
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Watch settings")
    }

    /// Windows' `.pl-tabs`: a segmented control on the glass.
    private var tabs: some View {
        HStack(spacing: m.px(2)) {
            ForEach([(PlayerSession.Panel.subtitles, "Subtitles"), (.audio, "Audio"), (.picture, "Picture")],
                    id: \.0) { value, label in
                let active = value == tab
                Button { session.panel = value } label: {
                    Text(label).frame(maxWidth: .infinity)
                }
                .buttonStyle(PillStyle(
                    foreground: active ? (theme == .contrast ? theme.accentText : theme == .light ? .black : .white)
                                       : DockTokens(theme).muted,
                    background: active ? (theme == .contrast ? theme.accent : theme.surface) : .clear,
                    height: m.px(36), radius: m.px(10), font: m.font(13, .bold), rim: nil))
                .frame(maxWidth: .infinity)
                .accessibilityAddTraits(active ? .isSelected : [])
            }
        }
        .padding(m.px(4))
        .background(DockTokens(theme).buttonHover, in: RoundedRectangle(cornerRadius: m.px(14), style: .continuous))
    }

    @ViewBuilder private var subtitles: some View {
        Choice(label: "Off", active: session.state.subtitleId == nil, m: m) { session.setSubtitleTrack(nil) }
        ForEach(session.state.subtitleTracks) { track in
            Choice(label: track.name, detail: track.detail, active: session.state.subtitleId == track.id, m: m) {
                session.setSubtitleTrack(track.id)
            }
        }
    }

    @ViewBuilder private var audio: some View {
        ForEach(session.state.audioTracks) { track in
            Choice(label: track.name, detail: track.detail, active: session.state.audioId == track.id, m: m) {
                session.setAudioTrack(track.id)
            }
        }
        if session.state.audioTracks.count <= 1 {
            Empty("This stream has one audio track.", m: m)
        }
    }

    @ViewBuilder private var picture: some View {
        // Live arrives at the rate it arrives; faster would only run out of buffer.
        if !session.isLive {
            Heading("Speed", m: m)
            Chips(m: m) {
                ForEach(session.menus.speeds, id: \.self) { speed in
                    Chip(label: speedLabel(speed), active: session.state.speed == speed, m: m) {
                        session.setSpeed(speed)
                    }
                }
            }
        }
        Heading("Aspect ratio", m: m)
        ForEach(session.menus.aspects, id: \.label) { choice in
            Choice(label: choice.label, detail: choice.detail, active: session.aspect == choice.aspect, m: m) {
                session.setAspect(choice.aspect)
            }
        }
        Heading("Sleep timer", m: m)
        if session.sleep.atUnix != nil {
            VStack(alignment: .leading, spacing: m.px(6)) {
                // Windows says "Stopping": on Apple the timer only ever stops playback.
                Text("Stopping in \(sleepLeft(session.sleep))").font(m.font(13, .regular))
                Chips(m: m) {
                    Chip(label: "+\(session.menus.sleepExtendMinutes) min", active: false, m: m) {
                        session.extendSleep()
                    }
                    Chip(label: "Off", active: false, m: m) { session.cancelSleep() }
                }
            }
        } else {
            Chips(m: m) {
                ForEach(session.menus.sleepMinutes, id: \.self) { minutes in
                    Chip(label: "\(minutes) min", active: false, m: m) { setSleep(.minutes(minutes: minutes)) }
                }
                // Only what can be answered: live has a programme, on demand a runtime.
                if session.isLive {
                    Chip(label: "End of programme", active: false, m: m) { setSleep(.endOfProgramme) }
                } else if session.state.duration > 0 {
                    Chip(label: endLabel, active: false, m: m) { setSleep(.endOfEpisode) }
                }
            }
        }
        if let sleepError {
            Text(sleepError).font(m.font(12, .semibold)).foregroundStyle(DockTokens.live)
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
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: m.px(12)) {
            Text(session.digits.isEmpty ? "–" : session.digits)
                .font(.system(size: m.px(44), weight: .heavy, design: .rounded))
                .monospacedDigit()
                .frame(maxWidth: .infinity, alignment: .center)
                .accessibilityLabel(session.digits.isEmpty ? "No number typed" : "Typed \(session.digits)")
            Grid(horizontalSpacing: m.px(8), verticalSpacing: m.px(8)) {
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
            Empty("Up to \(session.zap.maxDigits) digits. The channel changes when you stop typing.", m: m)
        }
    }

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
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(PillStyle(foreground: .white, background: DockTokens(theme).button, height: m.px(48),
                               radius: m.px(16), font: m.font(20, .bold), rim: nil))
    }
}

/// Windows' `.pl-heading`: a small grey capital line over a group.
private struct Heading: View {
    let text: String
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    init(_ text: String, m: DockMetrics) { self.text = text; self.m = m }
    var body: some View {
        Text(text.uppercased())
            .font(m.font(12))
            .tracking(0.5)
            .foregroundStyle(DockTokens(theme).muted)
            .padding(.top, m.px(10)).padding(.bottom, m.px(8))
            .accessibilityAddTraits(.isHeader)
    }
}

/// Windows' `.pl-empty`.
private struct Empty: View {
    let text: String
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    init(_ text: String, m: DockMetrics) { self.text = text; self.m = m }
    var body: some View {
        Text(text)
            .font(m.font(13, .regular))
            .foregroundStyle(DockTokens(theme).muted)
            .padding(.vertical, m.px(4))
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// Windows' `.pl-choice`: a row with its name, a line about it, and the accent's tick on
/// the one in use.
private struct Choice: View {
    let label: String
    var detail: String = ""
    let active: Bool
    let m: DockMetrics
    let pick: () -> Void

    var body: some View {
        Button(action: pick) {
            HStack(spacing: m.px(12)) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(label).font(m.font(15))
                    if !detail.isEmpty {
                        Text(detail).font(m.font(12, .semibold)).opacity(0.74)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Tick(m: m).opacity(active ? 1 : 0)
            }
        }
        .buttonStyle(ChoiceStyle(active: active, m: m))
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

/// Windows' `.pl-check`: the accent's round tick.
private struct Tick: View {
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    var body: some View {
        Image(systemName: "checkmark")
            .font(.system(size: m.px(12), weight: .heavy))
            .foregroundStyle(theme.accentText)
            .frame(width: m.px(26), height: m.px(26))
            .background(theme.accent, in: Circle())
            .accessibilityHidden(true)
    }
}

private struct ChoiceStyle: ButtonStyle {
    let active: Bool
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let dock = DockTokens(theme)
        configuration.label
            .padding(.vertical, m.px(12)).padding(.horizontal, m.px(14))
            .foregroundStyle(focused ? dock.inkText : dock.text)
            .background(focused ? dock.ink : active || configuration.isPressed ? dock.button : .clear,
                        in: RoundedRectangle(cornerRadius: m.px(14), style: .continuous))
            .contentShape(Rectangle())
            .scaleEffect(focused ? 1.03 : 1)
            .animation(.easeOut(duration: 0.15), value: focused)
    }
}

private struct Chips<Content: View>: View {
    let m: DockMetrics
    @ViewBuilder let content: Content
    var body: some View {
        FlowLayout(spacing: m.px(4)) { content }
            .padding(.bottom, m.px(8))
    }
}

/// Windows' `.pl-chip`: outlined, or the accent when it is the one in use.
private struct Chip: View {
    let label: String
    let active: Bool
    let m: DockMetrics
    let pick: () -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Button(action: pick) { Text(label) }
            .buttonStyle(PillStyle(foreground: active ? theme.accentText : .white,
                                   background: active ? theme.accent : .clear,
                                   height: m.px(28), radius: m.px(14), font: m.font(12, .bold),
                                   rim: active ? nil : (theme == .contrast ? .white : theme.borderStrong),
                                   padding: m.px(11), rimWidth: 1))
            .accessibilityAddTraits(active ? .isSelected : [])
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

/// Skip Intro / Recap / Credits (C22, Windows' `.pl-skip`). A marker learned from the
/// viewer's own skips says so.
private struct SkipButton: View {
    let session: PlayerSession
    let marker: SkipMarker
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let dock = DockTokens(theme)
        Button { session.skip(marker) } label: {
            HStack(spacing: m.px(8)) {
                Image(systemName: "forward.end.fill").font(.system(size: m.px(16), weight: .bold))
                Text(marker.label)
                if marker.source == .learned {
                    Text("Learned from your skips")
                        .font(m.font(12, .semibold))
                        .foregroundStyle(dock.accent)
                }
            }
        }
        .buttonStyle(SkipStyle(m: m))
        .focused(focus, equals: .skip)
        .accessibilityLabel(marker.label)
        .accessibilityHint(marker.source == .learned ? "Learned from your skips in this show" : "")
    }
}

private struct SkipStyle: ButtonStyle {
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let dock = DockTokens(theme)
        let label = configuration.label
            .font(m.font(15))
            .lineLimit(1)
            .padding(.vertical, m.px(13)).padding(.horizontal, m.px(24))
            .foregroundStyle(focused ? dock.inkText : dock.text)
        return Group {
            if focused {
                label.background(dock.ink, in: Capsule())
            } else {
                label.dockGlass(Capsule(), dock)
            }
        }
        .contentShape(Capsule())
        .opacity(configuration.isPressed ? 0.85 : 1)
        .scaleEffect(focused ? 1.08 : 1)
        .animation(.easeOut(duration: 0.15), value: focused)
    }
}

/// Up Next (C23, Windows' `UpNextCard`): Play now, Cancel, and a countdown when the show
/// plays on by itself.
private struct UpNextCard: View {
    let session: PlayerSession
    let episode: Episode
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme

    private var autoplay: Bool { session.aids?.prefs.autoplayNext == true }

    var body: some View {
        let dock = DockTokens(theme)
        TimelineView(.periodic(from: .now, by: 0.25)) { _ in
            let left = session.upNextLeft
            let total = Double(session.menus.upNextCountdownSecs)
            VStack(alignment: .leading, spacing: 0) {
                HStack(alignment: .top, spacing: m.px(12)) {
                    if let still = episode.still, let url = URL(string: still) {
                        AsyncImage(url: url) { $0.resizable().scaledToFill() } placeholder: { dock.button }
                            .frame(width: m.px(132), height: m.px(132) * 9 / 16)
                            .clipShape(RoundedRectangle(cornerRadius: m.px(14), style: .continuous))
                    }
                    VStack(alignment: .leading, spacing: m.px(2)) {
                        Text(autoplay ? "Up next in \(left)s" : "Up next")
                            .font(m.font(13))
                            .foregroundStyle(dock.accent)
                        Text(episode.title ?? "Episode \(episode.episode)").font(m.font(17)).lineLimit(1)
                        Text("S\(episode.season) E\(episode.episode)"
                             + (episode.runtimeMins.map { " · \(runtime($0))" } ?? ""))
                            .font(m.font(12, .bold))
                            .foregroundStyle(dock.muted)
                    }
                }
                .padding(m.px(14))
                HStack(spacing: m.px(8)) {
                    Button { session.playNext() } label: {
                        Label("Play now", systemImage: "play.fill")
                    }
                    .buttonStyle(PillStyle(foreground: theme.accentText, background: theme.accent,
                                           height: m.px(34), radius: m.px(12), font: m.font(13), rim: nil))
                    .focused(focus, equals: .upNextPlay)
                    Button("Cancel") { session.dismissUpNext() }
                        .buttonStyle(PillStyle(foreground: .white, background: dock.button,
                                               height: m.px(34), radius: m.px(12), font: m.font(13), rim: nil))
                        .focused(focus, equals: .upNextCancel)
                }
                .padding(.horizontal, m.px(14)).padding(.bottom, m.px(14))
                if autoplay {
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Rectangle().fill(dock.track)
                            Rectangle().fill(theme.accent)
                                .frame(width: geo.size.width * max(0, min(1, (total - Double(left)) / max(total, 1))))
                                .animation(.linear(duration: 0.25), value: left)
                        }
                    }
                    .frame(height: 4)
                    .accessibilityLabel("Countdown")
                }
            }
            .foregroundStyle(dock.text)
            .frame(width: min(m.px(380), m.width - 32), alignment: .leading)
            .clipShape(RoundedRectangle(cornerRadius: m.px(26), style: .continuous))
            .dockGlass(RoundedRectangle(cornerRadius: m.px(26), style: .continuous), dock)
        }
        #if os(tvOS)
        .focusSection()
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Up next")
    }

    private func runtime(_ minutes: UInt32) -> String {
        minutes >= 60 ? "\(minutes / 60)h \(minutes % 60)m" : "\(minutes)m"
    }
}

/// The last minute of the sleep timer (H20, Windows' `SleepWarning`): top centre, in the
/// app's own colours, with Keep watching and Cancel.
private struct SleepWarning: View {
    let session: PlayerSession
    var focus: FocusState<PlayerFocus?>.Binding
    let m: DockMetrics
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack {
            HStack(spacing: m.px(12)) {
                Text("Stopping in \(session.sleep.remainingSecs ?? 0)s")
                    .font(m.font(15, .semibold))
                    .monospacedDigit()
                button("play.fill", label: "Keep watching for another \(session.menus.sleepExtendMinutes) minutes") {
                    session.extendSleep()
                }
                .focused(focus, equals: .sleepKeep)
                button("xmark", label: "Cancel the sleep timer") { session.cancelSleep() }
            }
            .padding(.vertical, m.px(12)).padding(.horizontal, m.px(16))
            .background(theme.surface, in: RoundedRectangle(cornerRadius: m.px(18), style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: m.px(18), style: .continuous)
                    .strokeBorder(theme == .contrast ? Color.white : theme.borderStrong, lineWidth: theme == .contrast ? 2 : 1)
            }
            .shadow(color: .black.opacity(0.25), radius: 12, y: 8)
            .environment(\.colorScheme, theme.scheme)
            Spacer()
        }
        .padding(.top, m.px(24))
    }

    private func button(_ icon: String, label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: m.px(14), weight: .bold))
        }
        .buttonStyle(PillStyle(foreground: .primary, background: Color.primary.opacity(0.08),
                               height: m.px(36), radius: m.px(12), font: m.font(14), rim: nil, padding: m.px(11)))
        .accessibilityLabel(label)
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
