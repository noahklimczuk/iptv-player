import SwiftUI

/// Windows' top bar (App.tsx `TopBar`), over every tab: the "aurora" wordmark with its
/// accent dot, the page's name in faint display type beside it, and at the right the
/// search field, the light/dark switch, the clock and the profile button. Like Windows'
/// it is a scroll edge rather than a bar: the page scrolls up beneath a fade, and the
/// controls float on their own glass.
///
/// Where the screen is too narrow for all of it (an iPhone), the search field becomes a
/// round button first, then the page's name goes, then the clock.
struct AuroraTopBar: View {
    let title: String

    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    @Environment(\.openSearch) private var openSearch
    @AppStorage(AppearanceKey.theme) private var storedTheme: AuroraTheme = .dark
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    // `--topbar-h` (76; 96 in TV mode), `--fs-2xl`, `--fs-lg`, `--fs-md`, `--fs-xl`.
    #if os(tvOS)
    static let height: CGFloat = 96
    private let displaySize: CGFloat = 34
    private let clockSize: CGFloat = 22
    private let searchSize: CGFloat = 19
    private let initialSize: CGFloat = 26
    private let control = Daylight.px(44)
    #else
    static let height: CGFloat = 76
    private let displaySize: CGFloat = 26
    private let clockSize: CGFloat = 17
    private let searchSize: CGFloat = 15
    private let initialSize: CGFloat = 20
    private let control: CGFloat = 44
    #endif

    var body: some View {
        ViewThatFits(in: .horizontal) {
            bar(searchField: true, title: true, clock: true)
            bar(searchField: false, title: true, clock: true)
            bar(searchField: false, title: false, clock: true)
            bar(searchField: false, title: false, clock: false)
        }
        .padding(.horizontal, sideMargin)
        .frame(height: Self.height)
        .background { scrollEdge }
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private var sideMargin: CGFloat {
        #if os(iOS)
        sizeClass == .compact ? 20 : 36
        #else
        36
        #endif
    }

    private func bar(searchField: Bool, title showTitle: Bool, clock showClock: Bool) -> some View {
        HStack(spacing: 16) {
            wordmark
            if showTitle {
                Text(title)
                    .font(.display(size: displaySize))
                    .tracking(displaySize * -0.03)
                    .foregroundStyle(theme.textFaint)
                    .opacity(0.75)
                    .padding(.leading, 6)
                    .lineLimit(1)
                    .fixedSize()
            }
            Spacer(minLength: 0)
            HStack(spacing: 16) {
                search(field: searchField)
                themeToggle
                if showClock { clock }
                profileButton
            }
        }
    }

    /// The accent dot and "aurora", in display type a touch looser than a heading's.
    private var wordmark: some View {
        HStack(spacing: 10) {
            Circle().fill(theme.accent).frame(width: 14, height: 14)
            Text("aurora")
                .font(.display(size: displaySize))
                .tracking(displaySize * -0.03)
                .foregroundStyle(theme.text)
                .fixedSize()
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Aurora TV")
    }

    /// "Search channels, films, series" on its glass; a round button where it won't fit.
    @ViewBuilder private func search(field: Bool) -> some View {
        let action = openSearch ?? {}
        if field {
            Button(action: action) {
                HStack(spacing: 10) {
                    AuroraIcon(name: .search, size: 16)
                    Text("Search channels, films, series")
                        .font(.ui(size: searchSize, weight: .medium))
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    #if os(iOS)
                    // Windows' `Ctrl K`, as an iPad keyboard writes it.
                    if sizeClass != .compact {
                        Text("⌘ K")
                            .font(.ui(size: 11, weight: .heavy))
                            .padding(.horizontal, 7).padding(.vertical, 3)
                            .background(NavGlass.platter(theme), in: RoundedRectangle(cornerRadius: 6))
                    }
                    #endif
                }
                .padding(.leading, 16).padding(.trailing, 8)
                .frame(width: 310, height: control)
            }
            .buttonStyle(GlassPressStyle(shape: Capsule(), radius: control / 2))
            #if os(iOS)
            .keyboardShortcut("k", modifiers: .command)
            #endif
            .accessibilityLabel("Search")
        } else {
            Button(action: action) {
                AuroraIcon(name: .search, size: 19).frame(width: control, height: control)
            }
            .buttonStyle(GlassPressStyle(shape: Circle(), radius: control / 2))
            .accessibilityLabel("Search")
        }
    }

    /// Windows' `ThemeToggle`: light and dark, the sun or the moon on round glass.
    private var themeToggle: some View {
        let dark = storedTheme == .dark || storedTheme == .oled
        return Button {
            storedTheme = storedTheme == .light ? .dark : .light
        } label: {
            AuroraIcon(name: dark ? .sun : .moon, size: 19).frame(width: control, height: control)
        }
        .buttonStyle(GlassPressStyle(shape: Circle(), radius: control / 2))
        .accessibilityLabel("Dark mode")
        .accessibilityValue(dark ? "On" : "Off")
    }

    /// The time, in the viewer's 12- or 24-hour choice; Windows' redraws every 15 seconds.
    private var clock: some View {
        Ticking(every: 15) { context in
            Text(clockTime(context.date))
                .font(.ui(size: clockSize, weight: .heavy))
                .monospacedDigit()
                .foregroundStyle(theme.text)
                .fixedSize()
        }
    }

    /// The profile's initial on its colour (`artHue`), which opens "Who's watching?".
    private var profileButton: some View {
        let name = model.activeProfile?.name ?? "Me"
        return Button {
            model.picking = true
        } label: {
            Text(name.prefix(1).uppercased())
                .font(.display(size: initialSize))
                .foregroundStyle(.white)
                .frame(width: control, height: control)
                .background(ChannelTile.hue(name), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(ProfileButtonStyle(theme: theme))
        .accessibilityLabel("Switch profile (currently \(name))")
    }

    /// `.glass-bar::before`: the page behind fades out under the bar into the page's own
    /// colour (`--glass-edge-bg`), reaching 32 points past it. Windows also blurs it a
    /// little; Apple's materials add a grey tint of their own that Windows' edge doesn't
    /// have, so here it is the fade alone.
    private var scrollEdge: some View {
        Rectangle()
            .fill(LinearGradient(stops: [.init(color: theme.background, location: 0),
                                         .init(color: theme.background.opacity(0.82), location: 0.45),
                                         .init(color: theme.background.opacity(0.3), location: 0.75),
                                         .init(color: theme.background.opacity(0), location: 1)],
                                 startPoint: .top, endPoint: .bottom))
            .padding(.bottom, -32)
            .ignoresSafeArea(edges: .top)
            .allowsHitTesting(false)
    }
}

/// A control floating on the scroll edge: its own piece of Windows' glass, swelling a
/// little when pressed (`.glass-press`), with Windows' focus ring on a TV.
struct GlassPressStyle<S: InsettableShape>: ButtonStyle {
    let shape: S
    /// The shape's corner radius, for the focus ring around it.
    let radius: CGFloat

    func makeBody(configuration: Configuration) -> some View {
        Styled(configuration: configuration, shape: shape, radius: radius)
    }

    private struct Styled: View {
        let configuration: ButtonStyleConfiguration
        let shape: S
        let radius: CGFloat
        @Environment(\.auroraTheme) private var theme
        @Environment(\.isFocused) private var focused

        var body: some View {
            configuration.label
                .foregroundStyle(NavGlass.text(theme))
                .windowsGlass(shape, theme: theme)
                .contentShape(shape)
                .daylightFocusRing(focused, radius: radius, theme: theme)
                .scaleEffect(configuration.isPressed ? 1.02 : 1)
                .animation(.spring(duration: 0.2, bounce: 0.3), value: configuration.isPressed)
        }
    }
}

private struct ProfileButtonStyle: ButtonStyle {
    let theme: AuroraTheme

    func makeBody(configuration: Configuration) -> some View {
        Styled(configuration: configuration, theme: theme)
    }

    private struct Styled: View {
        let configuration: ButtonStyleConfiguration
        let theme: AuroraTheme
        @Environment(\.isFocused) private var focused

        var body: some View {
            configuration.label
                .daylightFocusRing(focused, radius: 14, theme: theme)
                .scaleEffect(configuration.isPressed ? 0.96 : 1)
                .animation(.spring(duration: 0.2), value: configuration.isPressed)
        }
    }
}
