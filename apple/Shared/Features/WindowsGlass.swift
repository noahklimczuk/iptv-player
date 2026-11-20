import SwiftUI

/// Windows' glass (styles/glass.css `.glass`): its own recipe rather than the system's
/// Liquid Glass, which reads lighter and frostier. A light blur with the page dimmed
/// beneath (lifted on Light), a thin tint, a sheen along the top, a specular rim brightest
/// at the top left, the hairline, and `--glass-shadow` drawn outside the shape only.
/// Solid in the contrast theme. The tab bar and the top bar's controls float on it.
extension View {
    func windowsGlass<S: InsettableShape>(_ shape: S, theme: AuroraTheme) -> some View {
        background { WindowsGlass(theme: theme, shape: shape) }
            .clipShape(shape)
            .overlay { WindowsGlassRim(theme: theme, shape: shape) }
            .background { GlassShadow(theme: theme, shape: shape) }
    }
}

/// `--glass-shadow`'s larger layer, masked to fall outside the shape so it never darkens
/// what shows through the glass.
private struct GlassShadow<S: Shape>: View {
    let theme: AuroraTheme
    let shape: S

    var body: some View {
        if let shadow {
            shape.fill(.black)
                .shadow(color: .black.opacity(shadow.opacity), radius: shadow.radius / 2, y: shadow.y)
                .mask {
                    ZStack {
                        Rectangle().fill(.black).padding(-80)
                        shape.fill(.black).blendMode(.destinationOut)
                    }
                    .compositingGroup()
                }
        }
    }

    private var shadow: (opacity: Double, radius: CGFloat, y: CGFloat)? {
        switch theme {
        case .light: (0.12, 30, 10)
        case .dark: (0.38, 34, 12)
        case .oled: (0.55, 34, 12)
        case .contrast: nil
        }
    }
}

/// `--glass-*` for the tab bar's `.glass`, at the default clarity (0.5).
struct GlassTokens {
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
struct WindowsGlass<S: Shape>: View {
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
struct WindowsGlassRim<S: InsettableShape>: View {
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

/// The glass's type and the tab bar's platter (`--glass-text`, `--glass-selected`, `--glass-platter`).
enum NavGlass {
    /// `--glass-text`: type on a control's glass.
    static func text(_ theme: AuroraTheme) -> Color { muted(theme) }

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
