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
            .font(.system(size: Daylight.px(fontSize), weight: .heavy))
            .labelStyle(.titleAndIcon)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
            .padding(.horizontal, iconOnly ? 0 : Daylight.px(padding))
            .frame(minWidth: iconOnly ? Daylight.px(height) : nil)
            .frame(height: Daylight.px(height))
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
                         iconOnly: Bool = false) -> DaylightButtonStyle {
        DaylightButtonStyle(variant: variant, size: size, iconOnly: iconOnly)
    }
}
