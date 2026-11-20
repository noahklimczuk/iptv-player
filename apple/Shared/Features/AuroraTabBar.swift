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
        .windowsGlass(shape, theme: theme)
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
            .fill(pressed ? NavGlass.platterPressed(theme) : NavGlass.platter(theme))
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
                .foregroundStyle(selected ? NavGlass.selected(theme) : NavGlass.muted(theme))
                // `:focus-visible`: two points of the selected colour, just inside.
                .overlay {
                    if focused {
                        shape.inset(by: 4).strokeBorder(NavGlass.selected(theme), lineWidth: 2)
                    }
                }
                .contentShape(shape)
                .animation(.easeOut(duration: 0.15), value: selected)
        }
    }
}
