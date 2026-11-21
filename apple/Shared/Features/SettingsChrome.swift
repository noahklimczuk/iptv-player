import SwiftUI

extension EnvironmentValues {
    /// Told when a section's heading comes on screen or leaves it, so the index can mark
    /// the section being read.
    @Entry var settingsHeadSeen: (String, Bool) -> Void = { _, _ in }
}

/// Windows' `settings-card-head`: a section's name inside its own card, in the display
/// face, with a line under it saying what the section is for. Each use is given
/// `.id(title)` where it is placed, for the index to scroll to: a list finds a row by the
/// id on the row, not one inside it.
struct SettingsHead: View {
    let title: String
    @Environment(\.settingsHeadSeen) private var seen

    init(_ title: String) { self.title = title }

    /// Not a Windows section: Windows switches profile from the top bar's avatar, and
    /// this is where the Apple apps do it.
    static let profile = "Profile"

    /// The sections, in page order, for the index (Windows' `SECTIONS`). Updates and
    /// Keyboard are not here (apple/README.md).
    static let order = [
        profile, "Providers", "Appearance", "Account and sync", "Parental controls", "Artwork and metadata",
        "Recommendations", "Privacy", "Filtering", "Pause live TV", "Library", "About",
        "Diagnostics",
    ]

    /// Windows' `SECTION_ICONS`, as SF Symbols.
    static let icons: [String: String] = [
        profile: "person.2",
        "Providers": "square.3.layers.3d",
        "Appearance": "sun.max",
        "Account and sync": "person.crop.circle",
        "Parental controls": "lock",
        "Artwork and metadata": "film",
        "Recommendations": "sparkles",
        "Privacy": "clock",
        "Filtering": "square.grid.2x2",
        "Pause live TV": "pause",
        "Library": "square.stack",
        "About": "info.circle",
        "Diagnostics": "exclamationmark.triangle",
    ]

    /// Windows' `SECTION_BLURBS`: absent where the section's own first paragraph says it.
    static let blurbs: [String: String] = [
        profile: "Who is watching, and the profiles on this device.",
        "Providers": "The playlists and Xtream lines your library is built from.",
        "Appearance": "How Aurora looks and moves.",
        "Account and sync": "Your Aurora account, and keeping every device in step.",
        "Parental controls": "What each profile may watch, and what needs the PIN.",
        "Privacy": "What Aurora remembers you watched, and for how long.",
        "Filtering": "What your library shows, and what it hides.",
        "Pause live TV": "Rewind and pause the channel you are watching.",
        "Library": "What your providers have given you.",
        "About": "This copy of Aurora, and the work it is built on.",
        "Diagnostics": "For when something is not working.",
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .displayFont(.title2)
            if let blurb = Self.blurbs[title] {
                Text(blurb)
                    .font(.ui(.subheadline))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.top, 6)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
        #if os(iOS)
        .listRowSeparator(.hidden, edges: .bottom)
        #endif
        #if os(tvOS)
        // In a scroll view every heading is made at once, so on screen is what counts.
        .onScrollVisibilityChange(threshold: 0.5) { seen(title, $0) }
        #else
        .onAppear { seen(title, true) }
        .onDisappear { seen(title, false) }
        #endif
    }
}


/// Windows' section index: a panel beside the page, the section being read on an ink
/// pill. Each entry scrolls the page to its section rather than opening a page of its own,
/// as the sections are all on the one page.
struct SettingsIndex: View {
    let titles: [String]
    let current: String
    let open: (String) -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 2) {
                // By position, not by title: the page's headings carry the titles as their
                // ids, and the index must not answer to them or `scrollTo` scrolls the index.
                ForEach(Array(titles.enumerated()), id: \.offset) { _, title in
                    entry(title)
                }
            }
            .padding(12)
            #if !os(tvOS)
            .background(theme.surface, in: RoundedRectangle(cornerRadius: 20))
            #endif
        }
        .scrollIndicators(.hidden)
        .frame(width: width)
        #if os(tvOS)
        .focusSection()
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Settings sections")
    }

    private func entry(_ title: String) -> some View {
        let active = title == current
        return Button { open(title) } label: {
            // The icons in a column of their own, so the names line up.
            HStack(spacing: 12) {
                Image(systemName: SettingsHead.icons[title] ?? "gearshape")
                    .frame(width: iconWidth)
                Text(title)
            }
                .fontWeight(.bold)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
                #if !os(tvOS)
                .font(.ui(.callout))
                .minimumScaleFactor(0.85)
                .padding(.horizontal, 14).padding(.vertical, 10)
                .foregroundStyle(active ? theme.background : Color.secondary)
                .background {
                    if active { RoundedRectangle(cornerRadius: 10).fill(Color.primary) }
                }
                .contentShape(Rectangle())
                #endif
        }
        #if os(tvOS)
        // The focus highlight is tvOS's own; the section being read keeps a filled pill.
        .buttonStyle(IndexEntryStyle(active: active))
        #else
        .buttonStyle(.plain)
        #endif
        .accessibilityAddTraits(active ? .isSelected : [])
    }

    private var iconWidth: CGFloat {
        #if os(tvOS)
        36
        #else
        22
        #endif
    }

    private var width: CGFloat {
        #if os(tvOS)
        500
        #else
        248
        #endif
    }
}

#if os(tvOS)
/// tvOS: white when focused, as every other button; else the ink pill for the section
/// being read, or nothing.
private struct IndexEntryStyle: ButtonStyle {
    let active: Bool
    @Environment(\.isFocused) private var focused

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.ui(.callout))
            .padding(.horizontal, 22).padding(.vertical, 14)
            .foregroundStyle(focused ? Color.black : active ? Color.primary : Color.secondary)
            .background {
                RoundedRectangle(cornerRadius: 14)
                    .fill(focused ? Color.white : active ? Color.primary.opacity(0.18) : .clear)
            }
            .scaleEffect(focused ? 1.04 : 1)
            .animation(.easeOut(duration: 0.15), value: focused)
    }
}
#endif

/// Windows' `Field`: the setting's name and a line about it, with its control beside them
/// where there is room and under them where there is not.
struct SettingsField<Control: View>: View {
    let label: String
    var hint: String?
    @ViewBuilder let control: Control

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 16) {
                words.frame(maxWidth: .infinity, alignment: .leading)
                control
            }
            VStack(alignment: .leading, spacing: 10) {
                words
                control
            }
        }
    }

    private var words: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).fontWeight(.heavy)
            if let hint {
                Text(hint).font(.ui(.subheadline)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

#if !os(tvOS)
/// Windows' small secondary `Button`: 36 points tall on the raised surface, its label
/// never wrapping. On a TV the system's bordered button stands in, for its focus.
struct SecondaryButtonStyle: ButtonStyle {
    @Environment(\.auroraTheme) private var theme
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.ui(.subheadline).weight(.bold))
            .labelStyle(.titleAndIcon)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, 14)
            .frame(height: 36)
            .foregroundStyle(Color.primary)
            .background(theme.surface, in: RoundedRectangle(cornerRadius: 10))
            .shadow(color: .black.opacity(0.12), radius: 1.5, y: 1)
            .opacity(enabled ? (configuration.isPressed ? 0.7 : 1) : 0.45)
    }
}
#endif
