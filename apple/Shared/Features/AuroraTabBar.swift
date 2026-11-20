import SwiftUI

/// Windows' tab bar (components/TabBar.tsx, `.glass` and `.nav-glass` in
/// styles/glass.css), on every Apple screen in place of the system's: a floating glass bar
/// at the foot of the page, every tab the same width with Windows' own icon over its
/// label, and the current one in the selected colour on a soft platter that slides to
/// whichever tab is chosen. Pressing the bar swells the platter; dragging along it moves
/// the platter under the finger, and letting go there goes there. Where the screen is too
/// narrow for every tab it scrolls sideways instead, as Windows' does in a narrow window.
///
/// The glass is Windows' own recipe rather than the system's Liquid Glass, which reads
/// lighter and frostier: a light blur, the page dimmed beneath (lifted on Light), a thin
/// tint, a sheen along the top, and a specular rim brightest at the top left.
struct AuroraTabBar<Value: Hashable>: View {
    struct Item: Identifiable {
        let value: Value
        let label: String
        let icon: AuroraIcon.Name
        var id: Value { value }
    }

    let items: [Item]
    @Binding var selection: Value

    @Environment(\.auroraTheme) private var theme
    /// The bar is held down (`data-pressed`).
    @State private var pressed = false
    /// Where the platter's left edge is while it is dragged (`data-dragging`).
    @State private var dragX: CGFloat?

    // Windows' sizes: `--navpill-h` (72, 92 in TV mode), 84-wide tabs, the 21-point icon
    // and an 11-point label (`--fs-xs` less one; 14 in TV mode).
    #if os(tvOS)
    static var height: CGFloat { 92 }
    private let tabWidth = Daylight.px(84)
    private let iconSize = Daylight.px(21)
    private let labelSize: CGFloat = 14
    #else
    static var height: CGFloat { 72 }
    private let tabWidth: CGFloat = 84
    private let iconSize: CGFloat = 21
    private let labelSize: CGFloat = 11
    #endif
    /// `--r-pill`, and the bar's padding, which the platter sits inside.
    static var radius: CGFloat { 28 }
    private let inset: CGFloat = 6
    /// How far a press must travel before it is a drag (`DRAG_SLOP`).
    private let slop: CGFloat = 6

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Self.radius, style: .continuous)
        ViewThatFits(in: .horizontal) {
            row(draggable: true)
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) { row(draggable: false) }
                    .onAppear { proxy.scrollTo(selection, anchor: .center) }
                    .onChange(of: selection) { _, value in
                        withAnimation { proxy.scrollTo(value, anchor: .center) }
                    }
            }
        }
        .frame(height: Self.height)
        .background { WindowsGlass(theme: theme, shape: shape) }
        .clipShape(shape)
        .overlay { WindowsGlassRim(theme: theme, shape: shape) }
        // `--glass-shadow`, and the hairline just outside.
        // Drawn outside the bar only, so it never darkens what shows through the glass.
        .background {
            if theme != .contrast {
                shape.fill(.black)
                    .shadow(color: .black.opacity(glassShadow.opacity), radius: glassShadow.radius / 2, y: glassShadow.y)
                    .mask {
                        ZStack {
                            Rectangle().fill(.black).padding(-80)
                            shape.fill(.black).blendMode(.destinationOut)
                        }
                        .compositingGroup()
                    }
            }
        }
        // `maxWidth: calc(100vw - var(--sp-6) * 2)`, `--sp-6` being 32 (border-box, so
        // the bar's own padding is inside it).
        .padding(.horizontal, 32)
        .frame(maxWidth: .infinity)
        #if os(tvOS)
        .focusSection()
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Main")
    }

    private func row(draggable: Bool) -> some View {
        let index = items.firstIndex { $0.value == selection }
        let platterX = dragX ?? CGFloat(index ?? 0) * tabWidth
        return HStack(spacing: 0) {
            ForEach(items) { item in tab(item) }
        }
        .background(alignment: .leading) {
            if index != nil || dragX != nil {
                platter
                    .frame(width: tabWidth)
                    .offset(x: platterX)
                    // Dragged, it follows the finger exactly; the spring is for letting go.
                    .animation(dragX == nil ? .spring(duration: 0.32, bounce: 0.3) : nil, value: platterX)
            }
        }
        .padding(inset)
        #if os(iOS)
        .simultaneousGesture(draggable ? drag : nil)
        #endif
    }

    /// `.nav-platter`: the soft block under the current tab, which swells into a lens of
    /// its own while the bar is pressed.
    private var platter: some View {
        RoundedRectangle(cornerRadius: Self.radius - inset, style: .continuous)
            .fill(pressed ? NavColours.platterPressed(theme) : NavColours.platter(theme))
            .overlay {
                if pressed && theme != .contrast {
                    RoundedRectangle(cornerRadius: Self.radius - inset, style: .continuous)
                        .strokeBorder(LinearGradient(colors: [GlassTokens(theme).rim, GlassTokens(theme).rimLow],
                                                     startPoint: .topLeading, endPoint: .bottomTrailing), lineWidth: 1)
                }
            }
            .shadow(color: .black.opacity(pressed ? 0.16 : 0), radius: 9, y: 6)
            .scaleEffect(x: pressed ? 1.04 : 1, y: pressed ? 1.06 : 1)
            .animation(.spring(duration: 0.2, bounce: 0.3), value: pressed)
    }

    #if os(iOS)
    /// Press, drag along the bar, let go: Windows' pointer handling, for a finger.
    private var drag: some Gesture {
        DragGesture(minimumDistance: 0, coordinateSpace: .local)
            .onChanged { value in
                pressed = true
                guard abs(value.translation.width) >= slop || dragX != nil else { return }
                // Held inside the bar, centred under the finger.
                let x = value.location.x - inset - tabWidth / 2
                dragX = min(max(x, 0), tabWidth * CGFloat(items.count - 1))
            }
            .onEnded { value in
                defer { pressed = false }
                guard let x = dragX else { return }
                let nearest = Int(((x + tabWidth / 2) / tabWidth).rounded(.down))
                let chosen = items[min(max(nearest, 0), items.count - 1)].value
                withAnimation(.spring(duration: 0.32, bounce: 0.3)) {
                    dragX = nil
                    selection = chosen
                }
            }
    }
    #endif

    private func tab(_ item: Item) -> some View {
        let selected = item.value == selection && dragX == nil
        return Button { selection = item.value } label: { label(item) }
            .buttonStyle(TabStyle(selected: selected, radius: Self.radius - inset, theme: theme))
            .id(item.value)
            .accessibilityAddTraits(item.value == selection ? .isSelected : [])
    }

    private func label(_ item: Item) -> some View {
        VStack(spacing: 3) {
            AuroraIcon(name: item.icon, size: iconSize, strokeWidth: 2)
            Text(item.label)
                .font(.ui(size: labelSize, wght: 650))
                .tracking(labelSize * 0.01)
                .lineLimit(1)
                .fixedSize()
        }
        .padding(.horizontal, 8)
        .frame(width: tabWidth)
        .frame(maxHeight: .infinity)
    }

    /// `--glass-shadow`'s larger layer.
    private var glassShadow: (opacity: Double, radius: CGFloat, y: CGFloat) {
        switch theme {
        case .light: (0.12, 30, 10)
        case .dark: (0.38, 34, 12)
        case .oled: (0.55, 34, 12)
        case .contrast: (0, 0, 0)
        }
    }
}

/// `--glass-*` for the tab bar's `.glass`, at the default clarity (0.5).
private struct GlassTokens {
    let tint: Color
    /// What `--glass-filter`'s brightness does to the page behind: dimmed on the dark
    /// themes, lifted on Light.
    let shade: Color
    let sheen: Color
    let rim: Color
    let rimLow: Color
    let rimLine: Color
    let inner: Color
    let hairline: Color

    init(_ theme: AuroraTheme) {
        switch theme {
        case .light:
            tint = .white.opacity(0.34 * 0.6)
            shade = .white.opacity(0.22)
            sheen = .white.opacity(0.28)
            rim = .white.opacity(0.95); rimLow = .white.opacity(0.55); rimLine = .white.opacity(0.4)
            inner = .white.opacity(0.32); hairline = .black.opacity(0.07)
        case .dark, .contrast:
            tint = Color(red: 28 / 255, green: 28 / 255, blue: 32 / 255).opacity(0.3 * 0.6)
            shade = .black.opacity(0.3)
            sheen = .white.opacity(0.07)
            rim = .white.opacity(0.5); rimLow = .white.opacity(0.22); rimLine = .white.opacity(0.12)
            inner = .white.opacity(0.05); hairline = .black.opacity(0.35)
        case .oled:
            tint = Color.black.opacity(0.32 * 0.6)
            shade = .black.opacity(0.3)
            sheen = .white.opacity(0.06)
            rim = .white.opacity(0.45); rimLow = .white.opacity(0.2); rimLine = .white.opacity(0.12)
            inner = .white.opacity(0.04); hairline = .black.opacity(0.6)
        }
    }
}

/// `.glass`'s body: a light blur of the page, dimmed or lifted, tinted, with a sheen
/// along the top. Solid black in the contrast theme.
private struct WindowsGlass<S: Shape>: View {
    let theme: AuroraTheme
    let shape: S

    var body: some View {
        if theme == .contrast {
            shape.fill(.black)
        } else {
            let t = GlassTokens(theme)
            ZStack {
                shape.fill(.ultraThinMaterial)
                    .environment(\.colorScheme, theme == .light ? .light : .dark)
                shape.fill(t.shade)
                shape.fill(t.tint)
                shape.fill(LinearGradient(stops: [.init(color: t.sheen, location: 0),
                                                  .init(color: .clear, location: 0.55)],
                                          startPoint: .top, endPoint: .bottom))
            }
        }
    }
}

/// `.glass`'s rim: the hairline outside, the rim line inside, the specular edge brightest
/// at the top left and dimmer at the bottom right, and the soft glow within.
private struct WindowsGlassRim<S: InsettableShape>: View {
    let theme: AuroraTheme
    let shape: S

    var body: some View {
        if theme == .contrast {
            // The theme's own outline is its lift.
            shape.strokeBorder(.white, lineWidth: 2)
        } else {
            let t = GlassTokens(theme)
            ZStack {
                shape.strokeBorder(t.inner, lineWidth: 6).blur(radius: 6).clipShape(shape)
                shape.strokeBorder(t.rimLine, lineWidth: 0.5)
                shape.strokeBorder(LinearGradient(stops: [.init(color: t.rim, location: 0),
                                                          .init(color: .clear, location: 0.45),
                                                          .init(color: .clear, location: 0.55),
                                                          .init(color: t.rimLow, location: 1)],
                                                  startPoint: .topLeading, endPoint: .bottomTrailing),
                                   lineWidth: 1)
                shape.inset(by: -0.5).stroke(t.hairline, lineWidth: 0.5)
            }
            .allowsHitTesting(false)
        }
    }
}

/// `.nav-glass a` colours and the platter's.
private enum NavColours {
    /// `--glass-text-muted`.
    static func muted(_ theme: AuroraTheme) -> Color {
        switch theme {
        case .light: Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255)
        case .dark, .oled: Color(red: 0xF4 / 255, green: 0xF3 / 255, blue: 0xEF / 255)
        case .contrast: .white
        }
    }

    /// `--glass-selected`: the accent, lightened toward white on the dark themes.
    static func selected(_ theme: AuroraTheme) -> Color {
        switch theme {
        case .light: theme.accent
        case .dark, .oled: theme.accent.mix(with: .white, by: 0.45)
        case .contrast: .black
        }
    }

    /// `--glass-platter`.
    static func platter(_ theme: AuroraTheme) -> Color {
        switch theme {
        case .light: Color(red: 120 / 255, green: 120 / 255, blue: 128 / 255).opacity(0.16)
        case .dark, .oled: Color(red: 120 / 255, green: 120 / 255, blue: 128 / 255).opacity(0.36)
        case .contrast: Color(red: 1, green: 0xDD / 255, blue: 0)
        }
    }

    /// `--glass-platter-press`.
    static func platterPressed(_ theme: AuroraTheme) -> Color {
        switch theme {
        case .light: .white.opacity(0.5)
        case .dark, .oled: .white.opacity(0.16)
        case .contrast: platter(theme)
        }
    }
}

/// One tab: `.nav-glass a`, in the selected colour when it is the current one.
private struct TabStyle: ButtonStyle {
    let selected: Bool
    let radius: CGFloat
    let theme: AuroraTheme

    func makeBody(configuration: Configuration) -> some View {
        TabBody(configuration: configuration, selected: selected, radius: radius, theme: theme)
    }

    private struct TabBody: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        let radius: CGFloat
        let theme: AuroraTheme
        @Environment(\.isFocused) private var focused

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
            configuration.label
                .foregroundStyle(selected ? NavColours.selected(theme) : NavColours.muted(theme))
                // `:focus-visible`: two points of the selected colour, just inside.
                .overlay {
                    if focused {
                        shape.inset(by: 4).strokeBorder(NavColours.selected(theme), lineWidth: 2)
                    }
                }
                .contentShape(shape)
                .animation(.easeOut(duration: 0.15), value: selected)
        }
    }
}
