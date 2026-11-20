import SwiftUI

/// Settings → Appearance (Windows' `ui` store: `theme`, `animations`, `use24h`).
///
/// Kept on the device, as Windows keeps it in the window's storage: a phone in light
/// mode and a TV in OLED black are both right for the room each is in.
enum AuroraTheme: String, CaseIterable, Identifiable {
    case dark, oled, light, contrast
    var id: String { rawValue }

    /// Windows labels its buttons by capitalising the key, so "Oled".
    var label: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }

    var scheme: ColorScheme { self == .light ? .light : .dark }

    /// `--bg` in Windows' tokens.css.
    var background: Color {
        switch self {
        case .light: Color(red: 0xF4 / 255, green: 0xF3 / 255, blue: 0xEF / 255)
        case .dark: Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255)
        case .oled, .contrast: .black
        }
    }

    /// `--surface`: a raised panel, and the chosen segment.
    var surface: Color {
        switch self {
        case .light: .white
        case .dark: Color(red: 0x20 / 255, green: 0x20 / 255, blue: 0x26 / 255)
        case .oled: Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255)
        case .contrast: .black
        }
    }

    /// `--surface-sunk`: a well, such as behind a segmented control.
    var surfaceSunk: Color {
        switch self {
        case .light: Color(red: 0xE9 / 255, green: 0xE7 / 255, blue: 0xE0 / 255)
        case .dark: Color(red: 0x0F / 255, green: 0x0F / 255, blue: 0x12 / 255)
        case .oled, .contrast: .black
        }
    }

    /// `--accent`: Daylight's blue, and the contrast theme's yellow.
    var accent: Color {
        self == .contrast
            ? Color(red: 1, green: 0xDD / 255, blue: 0)
            : Color(red: 0x2B / 255, green: 0x4B / 255, blue: 1)
    }
}

extension AuroraTheme {
    /// `--accent-text`: type on the accent.
    var accentText: Color { self == .contrast ? .black : .white }

    /// `--border-strong`.
    var borderStrong: Color {
        switch self {
        case .light: Color(red: 0xD9 / 255, green: 0xD6 / 255, blue: 0xCC / 255)
        case .dark: Color(red: 0x3E / 255, green: 0x3E / 255, blue: 0x48 / 255)
        case .oled, .contrast: Color(red: 0x34 / 255, green: 0x34 / 255, blue: 0x3C / 255)
        }
    }
}

enum AppearanceKey {
    static let theme = "AuroraTheme"
    static let use24h = "AuroraUse24h"
    static let animations = "AuroraAnimations"
}

/// A time of day in the viewer's chosen style (Windows' `clockTime`): 21:30, or 9:30 PM
/// with the 24-hour clock off. On by default, as on Windows.
func clockTime(_ date: Date) -> String {
    let use24h = UserDefaults.standard.object(forKey: AppearanceKey.use24h) as? Bool ?? true
    return date.formatted(.dateTime.hour(.defaultDigits(amPM: .abbreviated)).minute()
        .locale(clockLocale(use24h)))
}

func clockTime(unix: Int64) -> String { clockTime(Date(timeIntervalSince1970: TimeInterval(unix))) }

/// The device's locale with its hour cycle set by the 24-hour switch, so every
/// `Text(date, format: .dateTime.hour().minute())` under it follows the setting.
func clockLocale(_ use24h: Bool) -> Locale {
    var components = Locale.Components(locale: .current)
    components.hourCycle = use24h ? .zeroToTwentyThree : .oneToTwelve
    return Locale(components: components)
}

private struct AuroraAppearance: ViewModifier {
    /// The app's root, which sets the scene's style and counts the switches. A sheet
    /// follows the count of the screen that presented it.
    let root: Bool
    @AppStorage(AppearanceKey.theme) private var theme: AuroraTheme = .dark
    @AppStorage(AppearanceKey.use24h) private var use24h = true
    @AppStorage(AppearanceKey.animations) private var animations = true
    @State private var switches = 0
    @Environment(\.auroraSwitches) private var presenterSwitches

    func body(content: Content) -> some View {
        let count = root ? switches : presenterSwitches
        content
            // A sheet is built afresh after a switch between light and dark (see below).
            .id(root ? 0 : count)
            #if os(tvOS)
            .preferredColorScheme(theme.scheme)
            #else
            // Not on tvOS: there a tint fills every button and hides its label, and the
            // platform's focus is the white lift rather than a colour.
            .tint(theme.accent)
            // Every list and form under here over the theme's own background rather than
            // the system's grouped grey, so Dark is Windows' charcoal and OLED its black.
            .scrollContentBackground(.hidden)
            // The scene's style, so every window and sheet follows. The glass of a bar
            // already on screen (the tab bar, a toolbar, an open sheet's navigation bar)
            // keeps the old scheme until the app is next brought back, whichever way the
            // style is set, so after a switch the tabs and any open sheet are built
            // afresh, once the new style is in place, and their bars with them.
            .onChange(of: theme.scheme, initial: true) { old, scheme in
                guard root else { return }
                let style: UIUserInterfaceStyle = scheme == .light ? .light : .dark
                for case let scene as UIWindowScene in UIApplication.shared.connectedScenes {
                    scene.traitOverrides.userInterfaceStyle = style
                }
                guard old != scheme else { return }
                AuroraRebuild.inProgress = true
                AuroraRebuild.returnToPicker = true
                DispatchQueue.main.async {
                    switches += 1
                    // Past the rebuild's appear and disappear callbacks.
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1) { AuroraRebuild.inProgress = false }
                }
            }
            #endif
            .background(theme.background.ignoresSafeArea())
            .environment(\.auroraTheme, theme)
            .environment(\.auroraSwitches, count)
            .environment(\.locale, clockLocale(use24h))
            // Animations off: transitions and springs land at once (Windows'
            // `data-motion="off"`).
            .transaction { if !animations { $0.disablesAnimations = true; $0.animation = nil } }
    }
}

/// True while the tabs and sheets are being built afresh for a new scheme, so a screen
/// can tell that from being left (Settings re-locks when left; not for this).
@MainActor enum AuroraRebuild {
    static var inProgress = false
    /// Settings, built afresh, scrolls back to the theme picker that was pressed; once.
    static var returnToPicker = false
}

private struct RebuildOnSwitch: ViewModifier {
    @Environment(\.auroraSwitches) private var switches
    func body(content: Content) -> some View { content.id(switches) }
}

extension EnvironmentValues {
    @Entry var auroraTheme: AuroraTheme = .dark
    /// How many times the scheme has switched between light and dark this run.
    @Entry var auroraSwitches = 0
}

extension View {
    /// The viewer's theme, clock and motion: at the top of the app (`root`), and on each
    /// sheet presented from it, which would otherwise keep the system's appearance.
    func auroraAppearance(root: Bool = false) -> some View { modifier(AuroraAppearance(root: root)) }

    /// Built afresh when the scheme switches between light and dark: the tabs, whose
    /// bars would otherwise keep the old one.
    func auroraRebuildOnSwitch() -> some View { modifier(RebuildOnSwitch()) }
}

/// Settings → Appearance. Windows' Field labels and hints, in its order. Interface
/// density and glass transparency are not here: an Apple TV is always the 10-foot UI
/// and a phone never is, and the system draws the glass (apple/README.md).
struct AppearanceSection: View {
    @AppStorage(AppearanceKey.theme) private var theme: AuroraTheme = .dark
    @AppStorage(AppearanceKey.use24h) private var use24h = true
    @AppStorage(AppearanceKey.animations) private var animations = true

    static let id = "appearance.theme"

    var body: some View {
        Section {
            SettingsHead("Appearance").id("Appearance")
            SettingsField(label: "Theme", hint: "Applies to every screen except the player.") {
                Segments(options: AuroraTheme.allCases.map { ($0, $0.label) }, selection: $theme)
            }
            .id(Self.id)
            Toggle(isOn: $animations) {
                Text("Animations").fontWeight(.heavy)
                Text("Disables transitions.")
            }
            Toggle(isOn: $use24h) {
                Text("24-hour clock").fontWeight(.heavy)
                Text("Off shows times as 9:30 PM rather than 21:30.")
            }
        }
    }
}

extension View {
    /// A tab's page title. On iOS the navigation bar's, which scrolls away with the page.
    /// Not on tvOS: there the bar's title stays put while the page scrolls under it and
    /// shows through the letters, so the page carries its own heading instead
    /// (`PageHeading`), as Windows' pages do with `dl-page-title`.
    func auroraTitle(_ title: String) -> some View { modifier(AuroraTitle(title: title, inBar: false)) }

    /// A pushed page's title, in the navigation bar on every platform.
    func auroraPageTitle(_ title: String) -> some View { modifier(AuroraTitle(title: title, inBar: true)) }
}

/// The title, and the page's background: a navigation stack paints the system's own
/// behind each page (black in dark mode), so without it Dark would be OLED black rather
/// than Windows' charcoal. Per page, as a pushed page has its own.
private struct AuroraTitle: ViewModifier {
    let title: String
    let inBar: Bool
    @Environment(\.auroraTheme) private var theme

    func body(content: Content) -> some View {
        // tvOS has no navigation container background to set.
        #if os(tvOS)
        if inBar { content.navigationTitle(title) } else { content }
        #else
        content.navigationTitle(title).containerBackground(theme.background, for: .navigation)
        #endif
    }
}

/// Windows' `dl-page-title`: the page's name as the first thing on it, scrolling with it.
/// tvOS only; on iOS the navigation bar's large title is the same thing.
struct PageHeading: View {
    let title: String
    init(_ title: String) { self.title = title }

    var body: some View {
        #if os(tvOS)
        Text(title)
            .font(.largeTitle.weight(.heavy))
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityAddTraits(.isHeader)
            .listRowBackground(Color.clear)
        #else
        EmptyView()
        #endif
    }
}
