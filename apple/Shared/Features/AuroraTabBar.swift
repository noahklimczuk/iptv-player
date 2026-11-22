import SwiftUI

/// Windows' tab bar (components/TabBar.tsx, `.glass` and `.nav-glass` in
/// styles/glass.css), on every Apple screen in place of the system's: a floating glass bar
/// at the foot of the page, every tab the same width with Windows' own icon over its
/// label, and the current one in the selected colour on a soft platter that slides to
/// whichever tab is chosen. Pressing the bar swells the platter; dragging along it moves
/// the platter under the finger, and letting go there goes there. Where the screen is too
/// narrow for every tab it scrolls sideways instead, as Windows' does in a narrow window.
/// A phone is narrower than any window, so there it fits the screen: the tabs that fit
/// share its width, and the rest sit under More, a panel of the same glass above the bar.
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
    #if os(tvOS)
    /// Which tab has the focus, if any: the page's Menu button sends it here.
    var focus: FocusState<Value?>.Binding
    #endif

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

    #if os(iOS)
    /// On a phone: as many tabs as fit the screen, and the rest under More, whose panel
    /// this opens and closes. `nil` keeps every tab, scrolling when they don't all fit,
    /// as on an iPad or in Windows' window.
    var more: Binding<Bool>? = nil
    /// The bar's width, which decides how many tabs a phone shows.
    @State private var barWidth: CGFloat = 0
    #endif

    /// A place in the bar: a tab, or the phone's More.
    private enum Slot: Identifiable {
        case tab(Item)
        case more
        var id: AnyHashable {
            switch self {
            case .tab(let item): AnyHashable(item.value)
            case .more: AnyHashable("aurora.more")
            }
        }
    }

    var body: some View {
        #if os(iOS)
        if let more {
            phoneBar(more)
        } else {
            fullBar
        }
        #else
        fullBar
        #endif
    }

    /// Every tab, Windows' width each; scrolling sideways where they don't all fit.
    private var fullBar: some View {
        let shape = RoundedRectangle(cornerRadius: Self.radius, style: .continuous)
        let slots = items.map(Slot.tab)
        return ViewThatFits(in: .horizontal) {
            row(slots, width: tabWidth, draggable: true)
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) { row(slots, width: tabWidth, draggable: false) }
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

    #if os(iOS)
    /// The phone's bar: the screen's width less a 16-point margin, its tabs sharing it,
    /// and More last for the ones that don't fit.
    private func phoneBar(_ open: Binding<Bool>) -> some View {
        let shape = RoundedRectangle(cornerRadius: Self.radius, style: .continuous)
        let split = phoneSplit
        let slots = split.shown.map(Slot.tab) + (split.extras.isEmpty ? [] : [.more])
        return row(slots, width: split.width, draggable: true, extras: split.extras)
            .frame(maxWidth: .infinity, alignment: .leading)
            .frame(height: Self.height)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { barWidth = $0 }
            .windowsGlass(shape, theme: theme)
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity)
            .overlay(alignment: .bottomTrailing) {
                if open.wrappedValue && !split.extras.isEmpty {
                    morePanel(split.extras, open)
                        .padding(.trailing, 16)
                        .offset(y: -(Self.height + 12))
                        .transition(.scale(scale: 0.92, anchor: .bottomTrailing).combined(with: .opacity))
                }
            }
            .animation(.spring(duration: 0.3, bounce: 0.2), value: open.wrappedValue)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Main")
    }

    /// The narrowest a phone's tab gets before the next one moves under More: room for
    /// "Live TV" at Windows' 11 points.
    private static var minPhoneSlot: CGFloat { 62 }

    private var phoneSplit: (shown: [Item], extras: [Item], width: CGFloat) {
        let available = (barWidth > 0 ? barWidth : 360) - inset * 2
        let fit = max(3, Int(available / Self.minPhoneSlot))
        if items.count <= fit {
            return (items, [], available / CGFloat(max(1, items.count)))
        }
        return (Array(items.prefix(fit - 1)), Array(items.dropFirst(fit - 1)), available / CGFloat(fit))
    }

    /// More's panel: the other tabs in the bar's own glass, the current one on the
    /// platter in the selected colour, as it would be in the bar.
    private func morePanel(_ extras: [Item], _ open: Binding<Bool>) -> some View {
        let shape = RoundedRectangle(cornerRadius: 24, style: .continuous)
        return VStack(spacing: 2) {
            ForEach(extras) { item in
                let selected = item.value == selection
                Button {
                    withAnimation(.spring(duration: 0.32, bounce: 0.3)) {
                        selection = item.value
                        open.wrappedValue = false
                    }
                } label: {
                    HStack(spacing: 12) {
                        AuroraIcon(name: item.icon, size: iconSize, strokeWidth: 2)
                        Text(item.label).font(.ui(size: 15, wght: 650))
                        Spacer(minLength: 0)
                        if selected {
                            AuroraIcon(name: .check, size: 16, strokeWidth: 2.4)
                        }
                    }
                    .foregroundStyle(selected ? NavGlass.selected(theme) : theme.text)
                    .padding(.horizontal, 14)
                    .frame(height: 48)
                    .background {
                        if selected {
                            RoundedRectangle(cornerRadius: 24 - inset, style: .continuous)
                                .fill(NavGlass.platter(theme))
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(inset)
        .frame(width: 232)
        .windowsGlass(shape, theme: theme)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("More")
    }
    #endif

    private func row(_ slots: [Slot], width: CGFloat, draggable: Bool, extras: [Item] = []) -> some View {
        let index = slots.firstIndex { slot in
            switch slot {
            case .tab(let item): item.value == selection
            case .more: extras.contains { $0.value == selection }
            }
        }
        let platterX = dragX ?? CGFloat(index ?? 0) * width
        return HStack(spacing: 0) {
            ForEach(slots) { slot in
                switch slot {
                case .tab(let item): tab(item, width: width)
                case .more: moreTab(extras, width: width)
                }
            }
        }
        .background(alignment: .leading) {
            if index != nil || dragX != nil {
                platter
                    .frame(width: width)
                    .offset(x: platterX)
                    // Dragged, it follows the finger exactly; the spring is for letting go.
                    .animation(dragX == nil ? .spring(duration: 0.32, bounce: 0.3) : nil, value: platterX)
            }
        }
        .padding(inset)
        #if os(iOS)
        .simultaneousGesture(draggable ? drag(slots, width: width) : nil)
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
    /// Letting go on More opens its panel.
    private func drag(_ slots: [Slot], width: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 0, coordinateSpace: .local)
            .onChanged { value in
                pressed = true
                guard abs(value.translation.width) >= slop || dragX != nil else { return }
                // Held inside the bar, centred under the finger.
                let x = value.location.x - inset - width / 2
                dragX = min(max(x, 0), width * CGFloat(slots.count - 1))
            }
            .onEnded { value in
                defer { pressed = false }
                guard let x = dragX else { return }
                let nearest = Int(((x + width / 2) / width).rounded(.down))
                let slot = slots[min(max(nearest, 0), slots.count - 1)]
                withAnimation(.spring(duration: 0.32, bounce: 0.3)) {
                    dragX = nil
                    switch slot {
                    case .tab(let item):
                        selection = item.value
                        more?.wrappedValue = false
                    case .more:
                        more?.wrappedValue = true
                    }
                }
            }
    }
    #endif

    private func tab(_ item: Item, width: CGFloat) -> some View {
        let selected = item.value == selection && dragX == nil
        return Button {
            selection = item.value
            #if os(iOS)
            more?.wrappedValue = false
            #endif
        } label: { label(icon: item.icon, text: item.label, width: width) }
            .buttonStyle(TabStyle(selected: selected, radius: Self.radius - inset, theme: theme))
            .id(item.value)
            #if os(tvOS)
            .focused(focus, equals: item.value)
            #endif
            .accessibilityAddTraits(item.value == selection ? .isSelected : [])
    }

    /// The phone's More: the page it holds when one of those is open, so the bar still
    /// says where you are; otherwise "More".
    @ViewBuilder
    private func moreTab(_ extras: [Item], width: CGFloat) -> some View {
        #if os(iOS)
        let current = extras.first { $0.value == selection }
        let isOpen = more?.wrappedValue ?? false
        Button {
            more?.wrappedValue.toggle()
        } label: {
            label(icon: current?.icon ?? .more, text: current?.label ?? "More", width: width)
        }
        .buttonStyle(TabStyle(selected: (current != nil || isOpen) && dragX == nil,
                              radius: Self.radius - inset, theme: theme))
        .accessibilityLabel("More")
        .accessibilityValue(current?.label ?? "")
        .accessibilityHint(isOpen ? "Closes the list of other pages" : "Shows the other pages")
        #endif
    }

    private func label(icon: AuroraIcon.Name, text: String, width: CGFloat) -> some View {
        VStack(spacing: 3) {
            AuroraIcon(name: icon, size: iconSize, strokeWidth: 2)
            Text(text)
                .font(.ui(size: labelSize, wght: 650))
                .tracking(labelSize * 0.01)
                .lineLimit(1)
                .minimumScaleFactor(0.85)
                .fixedSize(horizontal: width >= tabWidth, vertical: false)
        }
        .padding(.horizontal, width >= tabWidth ? 8 : 2)
        .frame(width: width)
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
