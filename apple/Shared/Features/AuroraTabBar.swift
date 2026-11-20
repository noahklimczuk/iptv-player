import SwiftUI

/// Windows' tab bar (components/TabBar.tsx, `.nav-glass` in styles/glass.css), on every
/// Apple screen in place of the system's: a floating glass bar at the foot of the page,
/// every tab the same width with Windows' own icon over its label, and the current one in
/// the selected colour on a soft platter that slides to whichever tab is chosen. Where the
/// screen is too narrow for them all it scrolls sideways, as Windows' does in a narrow
/// window, rather than wrapping or shrinking.
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
    @Namespace private var platter

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
    /// `--r-pill`, and the platter's radius inside the bar's 6-point padding.
    private let radius: CGFloat = 28
    private let inset: CGFloat = 6

    var body: some View {
        ViewThatFits(in: .horizontal) {
            row
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) { row }
                    .onAppear { proxy.scrollTo(selection, anchor: .center) }
                    .onChange(of: selection) { _, value in
                        withAnimation { proxy.scrollTo(value, anchor: .center) }
                    }
            }
        }
        .frame(height: Self.height)
        .background { material }
        .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        // `maxWidth: calc(100vw - var(--sp-6) * 2)`.
        .padding(.horizontal, 24)
        .frame(maxWidth: .infinity)
        #if os(tvOS)
        .focusSection()
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Main")
    }

    private var row: some View {
        HStack(spacing: 0) {
            ForEach(items) { item in tab(item) }
        }
        .padding(inset)
        .animation(.spring(duration: 0.32, bounce: 0.3), value: selection)
    }

    private func tab(_ item: Item) -> some View {
        let selected = item.value == selection
        let style = TabStyle(selected: selected, radius: radius - inset, platter: platter, theme: theme)
        return Button { selection = item.value } label: { label(item) }
            .buttonStyle(style)
            .id(item.value)
            .accessibilityAddTraits(selected ? .isSelected : [])
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

    /// `.glass`: the system's Liquid Glass, which Windows' bar imitates; solid in the
    /// contrast theme, as Windows' is.
    @ViewBuilder private var material: some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        if theme == .contrast {
            shape.fill(.black).overlay(shape.strokeBorder(.white, lineWidth: 1))
        } else {
            Color.clear.glassEffect(.regular, in: shape)
        }
    }
}

/// One tab: `.nav-glass a` and the `.nav-platter` under the current one.
private struct TabStyle: ButtonStyle {
    let selected: Bool
    let radius: CGFloat
    let platter: Namespace.ID
    let theme: AuroraTheme

    func makeBody(configuration: Configuration) -> some View {
        TabBody(configuration: configuration, selected: selected, radius: radius, platter: platter, theme: theme)
    }

    private struct TabBody: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        let radius: CGFloat
        let platter: Namespace.ID
        let theme: AuroraTheme
        @Environment(\.isFocused) private var focused

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
            configuration.label
                .foregroundStyle(selected ? selectedColour : mutedColour)
                .background {
                    if selected {
                        shape
                            .fill(configuration.isPressed ? platterPressed : platterColour)
                            .matchedGeometryEffect(id: "platter", in: platter)
                            // Pressed, the platter swells into a lens of its own.
                            .scaleEffect(x: configuration.isPressed ? 1.04 : 1, y: configuration.isPressed ? 1.06 : 1)
                    }
                }
                // `:focus-visible`: two points of the selected colour, just inside.
                .overlay {
                    if focused {
                        shape.inset(by: 4).strokeBorder(selectedColour, lineWidth: 2)
                    }
                }
                .contentShape(shape)
                .animation(.spring(duration: 0.2), value: configuration.isPressed)
        }

        /// `--glass-text-muted`.
        private var mutedColour: Color {
            switch theme {
            case .light: Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255)
            case .dark, .oled: Color(red: 0xF4 / 255, green: 0xF3 / 255, blue: 0xEF / 255)
            case .contrast: .white
            }
        }

        /// `--glass-selected`: the accent, lightened toward white on the dark themes.
        private var selectedColour: Color {
            switch theme {
            case .light: theme.accent
            case .dark, .oled: theme.accent.mix(with: .white, by: 0.45)
            case .contrast: .black
            }
        }

        /// `--glass-platter`.
        private var platterColour: Color {
            switch theme {
            case .light: Color(red: 120 / 255, green: 120 / 255, blue: 128 / 255).opacity(0.16)
            case .dark, .oled: Color(red: 120 / 255, green: 120 / 255, blue: 128 / 255).opacity(0.36)
            case .contrast: Color(red: 1, green: 0xDD / 255, blue: 0)
            }
        }

        /// `--glass-platter-press`.
        private var platterPressed: Color {
            switch theme {
            case .light: .white.opacity(0.5)
            case .dark, .oled: .white.opacity(0.16)
            case .contrast: platterColour
            }
        }
    }
}
