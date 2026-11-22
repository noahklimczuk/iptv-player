import SwiftUI

/// Windows' Daylight primitives that more than one page draws: its `Button`
/// (`Primitives.tsx`), the focus ring (`--focus-ring`), and the tokens they need.

extension AuroraTheme {
    /// `--text`.
    var text: Color {
        switch self {
        case .light: Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255)
        case .dark, .oled: Color(red: 0xF4 / 255, green: 0xF3 / 255, blue: 0xEF / 255)
        case .contrast: .white
        }
    }

    /// `--text-muted`.
    var textMuted: Color {
        switch self {
        case .light: Color(red: 0x5E / 255, green: 0x5E / 255, blue: 0x66 / 255)
        case .dark, .oled: Color(red: 0xA9 / 255, green: 0xA9 / 255, blue: 0xB2 / 255)
        case .contrast: .white
        }
    }

    /// `--text-invert`: type on `--text`.
    var textInvert: Color {
        switch self {
        case .light: .white
        case .dark, .oled: Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255)
        case .contrast: .black
        }
    }

    /// `--border`.
    var border: Color {
        switch self {
        case .light: Color(red: 0xEC / 255, green: 0xEA / 255, blue: 0xE3 / 255)
        case .dark: Color(red: 0x2C / 255, green: 0x2C / 255, blue: 0x34 / 255)
        case .oled: Color(red: 0x22 / 255, green: 0x22 / 255, blue: 0x28 / 255)
        case .contrast: .white
        }
    }

    /// `--danger`.
    var danger: Color {
        self == .contrast ? .white : Color(red: 0xE5 / 255, green: 0x48 / 255, blue: 0x2A / 255)
    }

    /// `--danger-soft`: behind a warning.
    var dangerSoft: Color {
        switch self {
        case .light: Color(red: 0xFF / 255, green: 0xE9 / 255, blue: 0xE3 / 255)
        case .dark: Color(red: 0x3A / 255, green: 0x1F / 255, blue: 0x19 / 255)
        case .oled: Color(red: 0x2E / 255, green: 0x17 / 255, blue: 0x12 / 255)
        case .contrast: .black
        }
    }

    /// `--danger-border`.
    var dangerBorder: Color {
        switch self {
        case .light: Color(red: 0xFF / 255, green: 0xC4 / 255, blue: 0xB5 / 255)
        case .dark: Color(red: 0x7A / 255, green: 0x36 / 255, blue: 0x26 / 255)
        case .oled: Color(red: 0x6A / 255, green: 0x2E / 255, blue: 0x20 / 255)
        case .contrast: .white
        }
    }

    /// `--shadow-1`'s colour: a hairline under a raised surface (a full ring in Oled and
    /// Contrast, drawn by `daylightRaised`).
    var shadowOne: Color {
        switch self {
        case .light: Color(red: 0xE2 / 255, green: 0xE0 / 255, blue: 0xD8 / 255)
        case .dark: .black.opacity(0.5)
        case .oled: Color(red: 0x22 / 255, green: 0x22 / 255, blue: 0x28 / 255)
        case .contrast: .white
        }
    }
}

/// How much bigger the 10-foot UI draws Windows' pixel sizes (its `tv` density raises
/// `--fs-xl` from 20 to 26).
enum Daylight {
    #if os(tvOS)
    static let scale: CGFloat = 1.33
    #else
    static let scale: CGFloat = 1
    #endif

    static func px(_ v: CGFloat) -> CGFloat { (v * scale).rounded() }
}

extension View {
    /// Windows' raised `--surface` card with `--shadow-1` under it.
    func daylightRaised(_ theme: AuroraTheme, radius: CGFloat) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        return background {
            ZStack {
                if theme == .oled || theme == .contrast {
                    shape.fill(theme.surface)
                    shape.strokeBorder(theme.shadowOne, lineWidth: 1)
                } else {
                    shape.fill(theme.shadowOne).offset(y: 1)
                    shape.fill(theme.surface)
                }
            }
        }
    }

    /// Windows' `--focus-ring`: a 2-point gap of the page, then 3 points of accent. On a
    /// TV only, where focus is how anything is chosen.
    func daylightFocusRing(_ focused: Bool, radius: CGFloat, theme: AuroraTheme) -> some View {
        overlay {
            if focused {
                RoundedRectangle(cornerRadius: radius + 5, style: .continuous)
                    .strokeBorder(theme.accent, lineWidth: 3)
                    .padding(-5)
                RoundedRectangle(cornerRadius: radius + 2, style: .continuous)
                    .strokeBorder(theme.background, lineWidth: 2)
                    .padding(-2)
            }
        }
    }
}

/// Windows' `Button`: primary on the accent, secondary on the raised surface, ghost
/// bare, danger on red; small 36, medium 44, large 52 tall, never wrapping.
struct DaylightButtonStyle: ButtonStyle {
    enum Variant { case primary, secondary, ghost, danger }
    enum Size { case sm, md, lg }

    var variant: Variant = .secondary
    var size: Size = .md
    /// An icon-only button: square rather than padded.
    var iconOnly = false
    /// A long label that wraps onto more lines rather than widening the page past the
    /// screen (a phone's width is not Windows' window).
    var wraps = false

    @Environment(\.auroraTheme) private var theme
    @Environment(\.isEnabled) private var enabled
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let radius = Daylight.px(size == .lg ? 18 : 14)
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        let label = configuration.label
            .font(.ui(size: Daylight.px(fontSize), weight: .heavy))
            .labelStyle(.titleAndIcon)
            .lineLimit(wraps ? nil : 1)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: !wraps, vertical: wraps)
            .padding(.horizontal, iconOnly ? 0 : Daylight.px(padding))
            .padding(.vertical, wraps ? Daylight.px(8) : 0)
            .frame(minWidth: iconOnly ? Daylight.px(height) : nil)
            .frame(minHeight: wraps ? Daylight.px(height) : nil)
            .frame(height: wraps ? nil : Daylight.px(height))
            .foregroundStyle(foreground)
            .contentShape(shape)
        return Group {
            switch variant {
            case .secondary:
                label.daylightRaised(theme, radius: radius)
            case .ghost:
                label.background(configuration.isPressed ? theme.surface : .clear, in: shape)
            case .primary:
                label.background(theme.accent, in: shape)
            case .danger:
                label.background(theme.danger, in: shape)
            }
        }
        .daylightFocusRing(focused, radius: radius, theme: theme)
        .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.45)
        .scaleEffect(focused ? 1.04 : 1)
        .animation(.easeOut(duration: 0.12), value: focused)
    }

    private var foreground: Color {
        switch variant {
        case .primary: theme.accentText
        case .secondary: theme.text
        case .ghost: theme.textMuted
        case .danger: theme.textInvert
        }
    }

    private var height: CGFloat {
        switch size {
        case .sm: 36
        case .md: 44
        case .lg: 52
        }
    }

    private var padding: CGFloat {
        switch size {
        case .sm: 14
        case .md: 20
        case .lg: 26
        }
    }

    private var fontSize: CGFloat {
        switch size {
        case .sm: 13
        case .md: 15
        case .lg: 17
        }
    }
}

extension ButtonStyle where Self == DaylightButtonStyle {
    static func daylight(_ variant: DaylightButtonStyle.Variant = .secondary,
                         _ size: DaylightButtonStyle.Size = .md,
                         iconOnly: Bool = false, wraps: Bool = false) -> DaylightButtonStyle {
        DaylightButtonStyle(variant: variant, size: size, iconOnly: iconOnly, wraps: wraps)
    }
}

extension AuroraTheme {
    /// `--text-faint`: an empty cell's dash, what the provider calls a renamed row.
    var textFaint: Color {
        switch self {
        case .light: Color(red: 0x6A / 255, green: 0x6A / 255, blue: 0x72 / 255)
        case .dark, .oled: Color(red: 0x8B / 255, green: 0x8B / 255, blue: 0x95 / 255)
        case .contrast: Color(white: 0xE0 / 255)
        }
    }

    /// `--live`, and `--live-text` on it: the red-orange of on air and recording.
    var live: Color { self == .contrast ? .white : Color(red: 1, green: 0x5A / 255, blue: 0x36 / 255) }
    var liveText: Color { self == .contrast ? .black : .white }
    /// `--success`, and `--success-text`: the green of New.
    var success: Color { self == .contrast ? .white : Color(red: 0x14 / 255, green: 0xA3 / 255, blue: 0x7F / 255) }
    var successText: Color { self == .contrast ? .black : .white }
}

/// Windows' `Badge`: a small pill, on the sunk ground, the accent, or only outlined.
struct DaylightBadge: View {
    enum Tone { case neutral, accent, live, new, outline }
    let text: String
    var tone: Tone = .neutral
    @Environment(\.auroraTheme) private var theme

    /// Windows' `padding: '0 4px'`, for a badge inside a guide pill.
    var tight = false

    init(_ text: String, tone: Tone = .neutral, tight: Bool = false) {
        self.text = text
        self.tone = tone
        self.tight = tight
    }

    var body: some View {
        Text(tone == .live ? text.uppercased() : text)
            .font(.ui(size: Daylight.px(12), weight: .heavy))
            .lineLimit(1)
            .fixedSize()
            .padding(.vertical, tight ? 0 : Daylight.px(3)).padding(.horizontal, Daylight.px(tight ? 4 : 9))
            .foregroundStyle(foreground)
            .background {
                switch tone {
                case .neutral: Capsule().fill(theme.surfaceSunk)
                case .accent: Capsule().fill(theme.accent)
                case .live: Capsule().fill(theme.live)
                case .new: Capsule().fill(theme.success)
                case .outline: Capsule().strokeBorder(theme.borderStrong, lineWidth: 1.5)
                }
            }
    }

    private var foreground: Color {
        switch tone {
        case .neutral: theme.text
        case .accent: theme.accentText
        case .live: theme.liveText
        case .new: theme.successText
        case .outline: theme.textMuted
        }
    }
}

/// Windows' `dl-switch`: the sunk well, blue (Contrast: ringed white) when on, its white
/// knob sliding across.
struct DaylightSwitch: View {
    let isOn: Bool
    let label: String
    let action: () -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Button(action: action) {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule().fill(isOn ? theme.accent : theme.surfaceSunk)
                if theme == .contrast { Capsule().strokeBorder(Color.white, lineWidth: 2) }
                Circle()
                    .fill(theme == .contrast && isOn ? Color.black : Color.white)
                    .shadow(color: Color(red: 22 / 255, green: 22 / 255, blue: 26 / 255).opacity(0.25), radius: 1.5, y: 1)
                    .padding(Daylight.px(3))
            }
            .frame(width: Daylight.px(46), height: Daylight.px(28))
            .animation(.spring(duration: 0.25), value: isOn)
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(14)))
        .accessibilityLabel(label)
        .accessibilityValue(isOn ? "On" : "Off")
        .accessibilityAddTraits(.isToggle)
    }
}

/// A checkbox as Windows' fields draw one: the accent with a tick when on, an outlined
/// square when off.
struct DaylightCheckbox: View {
    let isOn: Bool
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Daylight.px(5), style: .continuous)
        ZStack {
            if isOn {
                shape.fill(theme.accent)
                Image(systemName: "checkmark")
                    .font(.ui(size: Daylight.px(12), weight: .heavy))
                    .foregroundStyle(theme.accentText)
            } else {
                shape.fill(theme.surface)
                shape.strokeBorder(theme.borderStrong, lineWidth: 1.5)
            }
        }
        .frame(width: Daylight.px(20), height: Daylight.px(20))
    }
}

/// One button of Windows' `dl-seg`: muted type in the sunk well, the active one raised on
/// the surface (Contrast: the accent). Put the buttons in `.daylightSegWell()`.
struct DaylightSegmentStyle: ButtonStyle {
    let active: Bool
    var height: CGFloat = 36
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        // `calc(var(--r-md) - 4px)`.
        let radius = Daylight.px(10)
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        configuration.label
            .font(.ui(size: Daylight.px(13), weight: .bold))
            .lineLimit(1)
            .padding(.horizontal, Daylight.px(14))
            .frame(height: Daylight.px(height))
            .foregroundStyle(active ? (theme == .contrast ? theme.accentText : theme.text) : theme.textMuted)
            .background {
                if active {
                    shape.fill(theme == .contrast ? theme.accent : theme.surface)
                        .shadow(color: Color(red: 22 / 255, green: 22 / 255, blue: 26 / 255).opacity(0.08), radius: 1, y: 1)
                }
            }
            .contentShape(shape)
            .daylightFocusRing(focused, radius: radius, theme: theme)
            .opacity(configuration.isPressed ? 0.8 : 1)
    }
}

extension View {
    /// Windows' `dl-seg` well: the sunk ground, 4 points round its buttons.
    func daylightSegWell() -> some View {
        modifier(SegWell())
    }
}

private struct SegWell: ViewModifier {
    @Environment(\.auroraTheme) private var theme
    func body(content: Content) -> some View {
        content
            .padding(Daylight.px(4))
            .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous))
            .fixedSize()
    }
}

/// Windows' `dl-filter`: a raised white chip, the accent when it is the one chosen.
struct DaylightFilterStyle: ButtonStyle {
    let selected: Bool
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        let radius = Daylight.px(14)
        configuration.label
            .font(.ui(size: Daylight.px(13), weight: .bold))
            .lineLimit(1)
            .foregroundStyle(selected ? theme.accentText : theme.text)
            .padding(.horizontal, Daylight.px(16))
            .frame(height: Daylight.px(40))
            .background {
                if selected {
                    RoundedRectangle(cornerRadius: radius).fill(theme.accent)
                } else {
                    Color.clear.daylightRaised(theme, radius: radius)
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: radius))
            .daylightFocusRing(focused, radius: radius, theme: theme)
            .opacity(configuration.isPressed ? 0.75 : 1)
    }
}

/// A bare button that shows nothing but Windows' focus ring when the remote is on it.
struct FocusRingStyle: ButtonStyle {
    var radius: CGFloat = Daylight.px(8)
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .contentShape(Rectangle())
            .daylightFocusRing(focused, radius: radius, theme: theme)
            .opacity(configuration.isPressed ? 0.75 : 1)
    }
}

/// Windows' `TextField` as a search box: the raised surface, an optional magnifier and
/// clear button, the accent's edge while typing. A TV draws its own pill behind a text
/// field, so there it is the same surface as a button that brings up the keyboard.
struct DaylightSearchField: View {
    @Binding var text: String
    let placeholder: String
    var width: CGFloat = 220
    var icon = true
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @State private var draft = ""
    @State private var editing = false
    #else
    @FocusState private var focused: Bool
    #endif

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous)
        #if os(tvOS)
        Button {
            draft = text
            editing = true
        } label: {
            HStack(spacing: 8) {
                if icon { Image(systemName: "magnifyingglass") }
                Text(text.isEmpty ? placeholder : text).lineLimit(1)
                Spacer(minLength: 0)
            }
            .font(.ui(size: Daylight.px(15), weight: .medium))
            .foregroundStyle(text.isEmpty ? theme.textFaint : theme.text)
            .padding(.vertical, Daylight.px(10)).padding(.horizontal, Daylight.px(14))
            .frame(width: Daylight.px(width), alignment: .leading)
            .background(theme.surface, in: shape)
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(14)))
        .accessibilityLabel(placeholder)
        .accessibilityValue(text)
        .alert(placeholder, isPresented: $editing) {
            TextField(placeholder, text: $draft)
            Button("Search") { text = draft }
            if !text.isEmpty { Button("Clear", role: .destructive) { text = "" } }
            Button("Cancel", role: .cancel) {}
        }
        #else
        HStack(spacing: 8) {
            if icon {
                Image(systemName: "magnifyingglass")
                    .font(.ui(size: 14, weight: .semibold))
                    .foregroundStyle(theme.textFaint)
            }
            TextField(placeholder, text: $text, prompt: Text(placeholder).foregroundStyle(theme.textFaint))
                .textFieldStyle(.plain)
                .font(.ui(size: 15, weight: .medium))
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .submitLabel(.search)
                .focused($focused)
            if !text.isEmpty {
                Button { text = "" } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(theme.textFaint)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.vertical, 10).padding(.horizontal, 14)
        .frame(width: width)
        .background(theme.surface, in: shape)
        .overlay(shape.strokeBorder(focused ? theme.accent : .clear, lineWidth: 1.5))
        .accessibilityElement(children: .contain)
        #endif
    }
}
