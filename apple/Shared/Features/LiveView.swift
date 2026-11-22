import SwiftUI

/// Live TV as Windows' `LivePage`: the channel groups in a panel beside one list (on an
/// iPhone, behind the page's title), and over the list the group's name and count, the
/// search, Number or A–Z, and Cards or List.
struct LiveView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var addingProvider = false
    /// Favourites first, then the profile's named lists (Windows' sidebar entries).
    @State private var lists: [FavoriteList] = []
    @State private var group: String?
    @State private var favoritesOnly = false
    /// Which favourites list: nil is the default Favourites.
    @State private var favoritesList: String?
    @State private var cards = false
    @State private var typed = ""
    @State private var letter: String?
    @State private var byName = false
    @State private var channels: [Channel] = []
    /// The first read for this selection finished: until then empty means "not read yet".
    @State private var loaded = false
    @State private var guide: [Int64: NowNext] = [:]
    @State private var asked: Set<Int64> = []
    @State private var playing: PlayerItem?
    @State private var guideFor: Channel?
    @State private var choosingGroup = false
    /// "Add to a new list…" is asking for a name for this channel.
    @State private var newListFor: Channel?
    @State private var newListName = ""
    @State private var renaming = false
    @State private var renameTo = ""
    @State private var confirmingDelete = false

    /// Windows' sidebar beside the list, where there is room for it.
    private var sidebarBeside: Bool {
        #if os(tvOS)
        true
        #else
        sizeClass == .regular
        #endif
    }

    private struct Selection: Hashable {
        let group: String?, favoritesOnly: Bool, list: String?, profile: Int64, library: Int
    }

    var body: some View {
        NavigationStack {
            Group {
                if sidebarBeside {
                    HStack(alignment: .top, spacing: 20) {
                        GroupSidebar(label: "Channel groups", groups: model.groups,
                                     pinned: pinned, selected: favoritesOnly ? nil : group) { pick($0) }
                            .frame(width: Daylight.px(230))
                        page
                    }
                    .padding(.horizontal, pagePadding)
                } else {
                    page.padding(.horizontal, 16)
                }
            }
            .background(theme.background)
            // The page's own heading names the list, as Windows' does; a phone's bar
            // title over it would say "Live TV" twice over two lines.
            .auroraTitle(sidebarBeside ? "Live TV" : "")
            #if os(iOS)
            .navigationBarTitleDisplayMode(sidebarBeside ? .automatic : .inline)
            #endif
            .toolbar {
                #if !os(tvOS)
                Button("Add provider", systemImage: "plus") { addingProvider = true }
                #endif
            }
            .sheet(isPresented: $addingProvider) {
                SetupWizard(additional: true, onFinished: { addingProvider = false }, onSkip: { addingProvider = false })
                    .auroraAppearance()
            }
            .sheet(isPresented: $choosingGroup) {
                NavigationStack {
                    GroupSidebar(label: "Channel groups", groups: model.groups, pinned: pinned,
                                 selected: favoritesOnly ? nil : group, fill: true) { pick($0); choosingGroup = false }
                        .padding(16)
                        .auroraPageTitle("Channel groups")
                        #if os(iOS)
                        .navigationBarTitleDisplayMode(.inline)
                        .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { choosingGroup = false } } }
                        #endif
                }
                .auroraAppearance()
            }
            .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
            .navigationDestination(item: $guideFor) { ChannelGuideView(channel: $0) }
            .task { await model.reload() }
            // The padlocks: what a parent locked, read again each visit (Windows' `LivePage`).
            .task { await model.loadParental() }
            .task(id: model.profileId) { lists = await model.favoriteLists() }
            .task(id: Selection(group: group, favoritesOnly: favoritesOnly, list: favoritesList,
                                profile: model.profileId, library: model.libraryVersion)) { await load() }
            .alert("Add to a new list", isPresented: Binding(get: { newListFor != nil }, set: { if !$0 { newListFor = nil } })) {
                TextField("List name", text: $newListName)
                Button("Add") {
                    guard let channel = newListFor else { return }
                    let name = newListName
                    Task { await addToNewList(channel, name: name) }
                }
                Button("Cancel", role: .cancel) { newListFor = nil }
            } message: {
                if let channel = newListFor { Text("Add \(channel.name) to a new list called:") }
            }
            .alert("Rename list", isPresented: $renaming) {
                TextField("List name", text: $renameTo)
                Button("Rename") { let name = renameTo; Task { await rename(to: name) } }
                Button("Cancel", role: .cancel) {}
            }
            .confirmationDialog("Delete \(listTitle)?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                Button("Delete list", role: .destructive) { Task { await deleteList() } }
            } message: {
                Text("The channels stay in your library; only this list goes.")
            }
        }
    }

    private var pagePadding: CGFloat {
        #if os(tvOS)
        0
        #else
        36
        #endif
    }

    // MARK: The sidebar's entries

    private var pinned: [GroupSidebar.Pinned] {
        var rows = [
            GroupSidebar.Pinned(label: "All channels", icon: "tv", active: group == nil && !favoritesOnly) {
                group = nil; favoritesOnly = false; favoritesList = nil
            },
            GroupSidebar.Pinned(label: "Favourites", icon: "heart", active: favoritesOnly && favoritesList == nil) {
                favoritesOnly = true; group = nil; favoritesList = nil
            },
        ]
        // One entry per named list, under the default.
        for list in lists where list.name != defaultFavorites && list.name != "Favourites" {
            rows.append(GroupSidebar.Pinned(label: "\(list.name) (\(list.channels))", icon: "heart",
                                            active: favoritesOnly && favoritesList == list.name) {
                favoritesOnly = true; group = nil; favoritesList = list.name
            })
        }
        return rows
    }

    private func pick(_ next: String?) {
        group = next
        favoritesOnly = false
        favoritesList = nil
    }

    private var listTitle: String { favoritesList ?? "Favourites" }

    /// Which list this is: "Live TV" over one group of 482 left nothing saying which.
    private var heading: String {
        favoritesOnly ? listTitle : group ?? "All channels"
    }

    // MARK: The page

    private var page: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Daylight.px(16)) {
                #if os(tvOS)
                // A row rather than the toolbar, which cuts the label to "Add…vider".
                Button("Add provider", systemImage: "plus") { addingProvider = true }
                    .buttonStyle(.daylight(.secondary))
                #endif
                if let progress = model.importProgress {
                    ImportProgressRow(progress: progress)
                }
                if let report = model.lastImport {
                    Text("Imported \(report.channels) channels, \(report.movies) films and \(report.series) series.")
                        .foregroundStyle(theme.textMuted)
                }
                header
                content
            }
            .padding(.bottom, Daylight.px(24))
            .foregroundStyle(theme.text)
        }
        .frame(maxWidth: .infinity)
    }

    private var header: some View {
        FlowLayout(spacing: Daylight.px(12)) {
            title
            DaylightSearchField(text: $typed, placeholder: "Search channels…")
            Segments(options: [(false, "Number"), (true, "A–Z")], selection: $byName)
                .accessibilityLabel("Order")
            Segments(options: [(true, "Cards"), (false, "List")], selection: $cards)
                .accessibilityLabel("View")
            if favoritesOnly, favoritesList != nil {
                Menu {
                    Button("Rename list", systemImage: "pencil") { renameTo = listTitle; renaming = true }
                    Button("Delete list", systemImage: "trash", role: .destructive) { confirmingDelete = true }
                } label: {
                    Image(systemName: "ellipsis")
                }
                .menuStyle(.button)
                .buttonStyle(.daylight(.ghost, .sm, iconOnly: true))
                .accessibilityLabel("List")
            }
        }
        .onChange(of: byName) { _, name in if !name { letter = nil } }
    }

    /// Windows' `dl-page-title` and the count beside it, faint. On an iPhone, where the
    /// sidebar has no room, the title opens it.
    @ViewBuilder private var title: some View {
        let words = HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(heading)
                .displayFont(size: Daylight.px(34))
                .lineLimit(1)
            if loaded {
                Text(visible.count.formatted())
                    .displayFont(size: Daylight.px(34))
                    .monospacedDigit()
                    .foregroundStyle(theme.textFaint.opacity(0.6))
            }
            if !sidebarBeside {
                Image(systemName: "chevron.down")
                    .font(.ui(size: 17, weight: .bold))
                    .foregroundStyle(theme.textMuted)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
        if sidebarBeside {
            words
        } else {
            Button { choosingGroup = true } label: { words }
                .buttonStyle(.plain)
                .accessibilityHint("Choose a channel group")
        }
    }

    @ViewBuilder private var content: some View {
        if model.providers.isEmpty && model.importProgress == nil && loaded {
            DaylightEmpty(icon: "tv", title: "No channels", message: emptyReason)
        } else if !loaded {
            VStack(spacing: 8) {
                ForEach(0..<8, id: \.self) { _ in
                    RoundedRectangle(cornerRadius: Daylight.px(18), style: .continuous)
                        .fill(theme.surfaceSunk)
                        .frame(height: Daylight.px(68))
                }
            }
        } else if channels.isEmpty {
            DaylightEmpty(icon: favoritesOnly ? "heart" : "tv",
                          title: favoritesOnly ? (favoritesList == nil ? "No favourite channels yet" : "\(listTitle) is empty")
                                               : "No channels",
                          message: favoritesOnly ? heartHint : emptyReason)
        } else if visible.isEmpty {
            // Channels exist, but not after this search or this letter: clear the box.
            DaylightEmpty(icon: "magnifyingglass", title: "No channel matches", message: noMatch) {
                Button("Clear search") { typed = ""; letter = nil }
                    .buttonStyle(.daylight(.secondary))
            }
        } else {
            VStack(alignment: .leading, spacing: Daylight.px(12)) {
                LetterBar(selection: $letter)
                    // A letter only means something against A–Z, as on Windows.
                    .disabled(!byName)
                    .opacity(byName ? 1 : 0.35)
                if cards { grid } else { list }
            }
        }
    }

    private var list: some View {
        LazyVStack(spacing: 8) {
            ForEach(Array(visible.enumerated()), id: \.element.id) { index, channel in
                Button { playing = .channel(id: channel.id, name: channel.name) } label: {
                    LiveChannelRow(channel: channel, guide: guide[channel.id],
                                   locked: model.isLocked(channelId: channel.id, group: channel.group),
                                   toggleFavorite: heart(channel),
                                   addToNewList: sidebarBeside ? newList(channel) : nil,
                                   compact: !sidebarBeside)
                }
                .buttonStyle(ChannelCardStyle(radius: Daylight.px(18)))
                .accessibilityLabel("Watch \(channel.name)")
                .contextMenu { menu(for: channel) }
                .onAppear { want(from: index) }
            }
        }
    }

    private var grid: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: Daylight.px(sidebarBeside ? 250 : 160)), spacing: 16)],
                  spacing: 16) {
            ForEach(Array(visible.enumerated()), id: \.element.id) { index, channel in
                Button { playing = .channel(id: channel.id, name: channel.name) } label: {
                    LiveChannelCard(channel: channel, guide: guide[channel.id],
                                    toggleFavorite: heart(channel))
                }
                .buttonStyle(ChannelCardStyle(radius: Daylight.px(24)))
                .accessibilityLabel("Watch \(channel.name)")
                .contextMenu { menu(for: channel) }
                .onAppear { want(from: index) }
            }
        }
    }

    /// Windows' `visible`: the search, then the letter, then the order.
    private var visible: [Channel] {
        var rows = channels
        let needle = typed.trimmingCharacters(in: .whitespaces).lowercased()
        if !needle.isEmpty { rows = rows.filter { $0.name.lowercased().contains(needle) } }
        if let letter {
            let first: (Channel) -> String = { String($0.name.trimmingCharacters(in: .whitespaces).prefix(1)).uppercased() }
            rows = letter == "#"
                ? rows.filter { !("A"..."Z").contains(first($0)) || first($0).isEmpty }
                : rows.filter { first($0) == letter }
        }
        if byName { rows.sort { $0.name.localizedCompare($1.name) == .orderedAscending } }
        return rows
    }

    private var noMatch: String {
        let text = typed.trimmingCharacters(in: .whitespaces)
        if let letter, !text.isEmpty { return "Nothing here matches “\(text)” and starts with \(letter)." }
        if let letter {
            return "No channel in this group starts with \(letter == "#" ? "a number or symbol" : letter)."
        }
        return "Nothing here matches “\(text)”."
    }

    @ViewBuilder private func menu(for channel: Channel) -> some View {
        Button(channel.hasCatchup ? "Guide and catch-up" : "Guide", systemImage: "calendar") {
            guideFor = channel
        }
        Button(channel.favorite ? "Remove from \(listTitle)" : "Add to \(listTitle)",
               systemImage: channel.favorite ? "heart.slash" : "heart") {
            Task { await toggle(channel) }
        }
        Button("Add to a new list…", systemImage: "plus") {
            newListName = ""
            newListFor = channel
        }
        // The order of a list is the viewer's own (Windows drags it); everywhere else
        // it is the provider's numbering.
        if favoritesOnly, !byName, typed.isEmpty, letter == nil,
           let i = channels.firstIndex(where: { $0.id == channel.id }) {
            if i > 0 {
                Button("Move up", systemImage: "arrow.up") { Task { await move(from: i, to: i - 1) } }
            }
            if i < channels.count - 1 {
                Button("Move down", systemImage: "arrow.down") { Task { await move(from: i, to: i + 1) } }
            }
        }
    }

    /// The heart beside each row, where a row has room for a second thing to press. A TV
    /// row is one thing to focus, so there the heart only shows; the long press changes it.
    private func heart(_ channel: Channel) -> (() -> Void)? {
        #if os(tvOS)
        nil
        #else
        { Task { await toggle(channel) } }
        #endif
    }

    private func newList(_ channel: Channel) -> (() -> Void)? {
        #if os(tvOS)
        nil
        #else
        { newListName = ""; newListFor = channel }
        #endif
    }

    private var heartHint: String {
        #if os(tvOS)
        "Press and hold any channel in the list to add it here."
        #else
        "Use the heart beside any channel in the list to add it here."
        #endif
    }

    /// Windows' `emptyReason`: why a list with no channels is empty.
    private var emptyReason: String {
        let providers = model.providers
        if providers.isEmpty { return "Add a provider in Settings to populate your channel list." }
        let imported = providers.reduce(0) { $0 + Int($1.channelCount) }
        if imported == 0 {
            let names = providers.map(\.name).joined(separator: ", ")
            return "\(names) imported no live channels. Some accounts carry films and series "
                + "but no live streams; if yours should have them, press Refresh next to the "
                + "provider in Settings and check the count it reports."
        }
        return "Your library has \(imported) channels, but none are visible here. Check "
            + "the group filter above, and Settings → Filtering for English-only and "
            + "hide-duplicates."
    }

    // MARK: Reading

    private func load() async {
        loaded = false
        letter = nil
        let rows = await model.channels(group: group, favoritesOnly: favoritesOnly, list: favoritesList,
                                        limit: 100_000)
        guard !Task.isCancelled else { return }
        channels = rows
        guide = [:]
        asked = []
        loaded = true
    }

    /// Now and next for the rows coming on screen, forty at a time: a library runs to
    /// thousands of channels, and the guide is only wanted for what is seen.
    private func want(from index: Int) {
        let rows = visible
        guard rows.indices.contains(index), !asked.contains(rows[index].id) else { return }
        let ids = rows[index..<min(rows.count, index + 40)].map(\.id).filter { !asked.contains($0) }
        asked.formUnion(ids)
        Task {
            let answer = await model.nowNext(channelIds: ids)
            for row in answer { guide[row.channelId] = row }
        }
    }

    private func toggle(_ channel: Channel) async {
        guard let now = await model.toggleFavorite(channelId: channel.id, list: favoritesList) else { return }
        if favoritesOnly && !now {
            channels.removeAll { $0.id == channel.id }
        } else if let i = channels.firstIndex(where: { $0.id == channel.id }) {
            channels[i].favorite = now
        }
        lists = await model.favoriteLists()
    }

    /// A list exists once something is in it, so making one and filling it are the same
    /// step, as on Windows.
    private func addToNewList(_ channel: Channel, name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != favoritesList else { return }
        _ = await model.toggleFavorite(channelId: channel.id, list: name)
        lists = await model.favoriteLists()
    }

    private func move(from: Int, to: Int) async {
        var next = channels
        next.insert(next.remove(at: from), at: to)
        channels = next
        await model.reorderFavorites(list: favoritesList, channelIds: next.map(\.id))
    }

    private func rename(to name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let from = favoritesList, !name.isEmpty, name != from else { return }
        await model.renameFavoriteList(from, to: name)
        lists = await model.favoriteLists()
        favoritesList = name
    }

    private func deleteList() async {
        guard let name = favoritesList else { return }
        await model.deleteFavoriteList(name)
        lists = await model.favoriteLists()
        favoritesOnly = false
        favoritesList = nil
    }
}

/// The default list's stored name (`aurora_db::repo::lists::FAVORITES`), shown as
/// "Favourites".
let defaultFavorites = "Favorites"

private struct ImportProgressRow: View {
    let progress: AppModel.ImportProgress

    var body: some View {
        VStack(alignment: .leading) {
            Text(progress.phase.label)
            if progress.total > 0 {
                ProgressView(value: Double(progress.total == 0 ? 0 : progress.done), total: Double(progress.total))
            } else {
                ProgressView()
            }
        }
    }
}

/// Windows' `GroupSidebar`: a raised panel with a filter once there are enough groups,
/// the pinned entries (All channels, Favourites, the named lists), a rule, and the
/// provider's groups with their counts; the chosen one in ink.
struct GroupSidebar: View {
    struct Pinned {
        let label: String
        let icon: String
        let active: Bool
        let action: () -> Void
    }

    let label: String
    let groups: [ChannelGroup]
    let pinned: [Pinned]
    let selected: String?
    /// Fill the sheet it is in, rather than sizing to the page beside it.
    var fill = false
    let choose: (String?) -> Void
    @Environment(\.auroraTheme) private var theme
    @State private var filter = ""

    private var shown: [ChannelGroup] {
        let needle = filter.trimmingCharacters(in: .whitespaces).lowercased()
        return needle.isEmpty ? groups : groups.filter { $0.name.lowercased().contains(needle) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if groups.count > 12 {
                DaylightSearchField(text: $filter, placeholder: "Filter groups…", width: fill ? 360 : 206)
            }
            VStack(spacing: 2) {
                ForEach(pinned, id: \.label) { row in
                    entry(row.label, icon: row.icon, count: nil, active: row.active, action: row.action)
                }
            }
            if !groups.isEmpty {
                Rectangle().fill(theme.border).frame(height: 1).padding(.horizontal, 6).padding(.vertical, 2)
            }
            ScrollView {
                LazyVStack(spacing: 2) {
                    if !groups.isEmpty, shown.isEmpty {
                        Text("No group matches “\(filter.trimmingCharacters(in: .whitespaces))”.")
                            .font(.ui(size: Daylight.px(13)))
                            .foregroundStyle(theme.textFaint)
                            .padding(6)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    ForEach(shown, id: \.name) { group in
                        entry(group.name, icon: nil, count: group.count, active: selected == group.name) {
                            choose(selected == group.name ? nil : group.name)
                        }
                    }
                }
            }
            #if os(tvOS)
            .focusSection()
            #endif
        }
        .padding(12)
        .frame(maxHeight: fill ? .infinity : nil, alignment: .top)
        .daylightRaised(theme, radius: Daylight.px(24))
        .foregroundStyle(theme.text)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }

    private func entry(_ title: String, icon: String?, count: Int64?, active: Bool,
                       action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if let icon { Image(systemName: icon).font(.ui(size: Daylight.px(14), weight: .semibold)) }
                Text(title).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                if let count, count > 0 {
                    Text(count.formatted())
                        .font(.ui(size: Daylight.px(12), weight: .bold))
                        .monospacedDigit()
                        .opacity(0.6)
                }
            }
            .font(.ui(size: Daylight.px(15), weight: .bold))
            .padding(.horizontal, 14)
            .frame(height: Daylight.px(37))
            .foregroundStyle(active ? theme.background : theme.text)
            .background(active ? theme.text : .clear, in: RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(14)))
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

/// A channel row or card as a button: the raised surface, Windows' focus ring on a TV.
private struct ChannelCardStyle: ButtonStyle {
    let radius: CGFloat
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    @Environment(\.isFocused) private var focused
    #else
    private let focused = false
    #endif

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .daylightRaised(theme, radius: radius)
            .contentShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
            .daylightFocusRing(focused, radius: radius, theme: theme)
            .scaleEffect(focused ? 1.02 : configuration.isPressed ? 0.98 : 1)
            .animation(.easeOut(duration: 0.12), value: focused)
    }
}

/// Windows' progress bar: a 4-point track in the type's colour, filled with the accent.
struct DaylightProgress: View {
    let value: Double
    var height: CGFloat = 4
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(theme.text.opacity(0.11))
                Capsule().fill(theme.accent).frame(width: geo.size.width * max(0, min(1, value)))
            }
        }
        .frame(height: height)
        .accessibilityLabel("Programme progress")
        .accessibilityValue("\(Int(value * 100)) percent")
    }
}

/// Windows' empty state: an icon in a sunk square, the title, the reason, and an action.
struct DaylightEmpty<Action: View>: View {
    let icon: String
    let title: String
    let message: String
    @ViewBuilder var action: Action
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack(spacing: Daylight.px(12)) {
            Image(systemName: icon)
                .font(.ui(size: Daylight.px(26), weight: .semibold))
                .frame(width: Daylight.px(64), height: Daylight.px(64))
                .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: Daylight.px(18), style: .continuous))
            Text(title).displayFont(size: Daylight.px(20))
            Text(message)
                .font(.ui(size: Daylight.px(15)))
                .foregroundStyle(theme.textMuted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: Daylight.px(460))
                .fixedSize(horizontal: false, vertical: true)
            action
        }
        .foregroundStyle(theme.text)
        .frame(maxWidth: .infinity)
        .padding(.vertical, Daylight.px(48))
    }
}

extension DaylightEmpty where Action == EmptyView {
    init(icon: String, title: String, message: String) {
        self.init(icon: icon, title: title, message: message) { EmptyView() }
    }
}

private func programmeProgress(_ p: Programme?) -> Double {
    guard let p else { return 0 }
    let now = Date().timeIntervalSince1970
    let span = Double(p.stop - p.start)
    guard span > 0 else { return 0 }
    return min(max((now - Double(p.start)) / span, 0), 1)
}

/// Windows' channel row (`LivePage`): the number, the logo, the name with its padlock and
/// quality, what is on (its start time and title, or "No guide data") and what is next,
/// how far through, then the Dropped and Catch-up badges, the heart, the new-list plus
/// and the play square.
private struct LiveChannelRow: View {
    let channel: Channel
    let guide: NowNext?
    var locked = false
    var toggleFavorite: (() -> Void)?
    var addToNewList: (() -> Void)?
    /// A phone's row: the name needs the room the play square and the new-list plus take,
    /// and the whole row plays anyway.
    var compact = false
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        HStack(spacing: compact ? 10 : 14) {
            Text(channel.number.map { String($0) } ?? "")
                .font(.ui(size: Daylight.px(15), weight: .heavy)).monospacedDigit()
                .foregroundStyle(theme.textFaint)
                .lineLimit(1).minimumScaleFactor(0.6)
                .frame(width: Daylight.px(40), alignment: .trailing)
            ChannelTile(name: channel.name, number: channel.number, logo: channel.logo, size: Daylight.px(44))
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(channel.name).font(.ui(size: Daylight.px(17), weight: .heavy)).lineLimit(1)
                    if locked {
                        Image(systemName: "lock.fill").font(.ui(size: Daylight.px(12)))
                            .accessibilityLabel("Locked: needs the parental PIN")
                    }
                    if let quality = channel.quality { DaylightBadge(quality) }
                }
                Text("\(guide?.now.map { "\(clockTime(unix: $0.start)) \($0.title)" } ?? "No guide data")\(Text(guide?.next.map { "  ·  Next: \($0.title)" } ?? "").foregroundStyle(theme.textFaint))")
                    .font(.ui(size: Daylight.px(13)))
                    .foregroundStyle(theme.textMuted)
                    .lineLimit(1)
                let pct = programmeProgress(guide?.now)
                if pct > 0 {
                    DaylightProgress(value: pct).frame(maxWidth: Daylight.px(260)).padding(.top, 4)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 6) {
                if channel.stale {
                    Text("Dropped")
                        .font(.ui(size: Daylight.px(12), weight: .heavy))
                        .foregroundStyle(Color.orange)
                        .padding(.vertical, 3).padding(.horizontal, 9)
                        .overlay(Capsule().strokeBorder(Color.orange, lineWidth: 1.5))
                        .accessibilityHint("Your provider no longer lists this channel")
                }
                if channel.hasCatchup { Badge(text: "Catch-up", style: .new) }
                if let toggleFavorite {
                    Button(action: toggleFavorite) {
                        Image(systemName: channel.favorite ? "heart.fill" : "heart")
                            .font(.ui(size: 16))
                            .foregroundStyle(channel.favorite ? theme.accent : theme.textFaint)
                            .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(channel.favorite ? "Remove \(channel.name) from favourites"
                                                         : "Add \(channel.name) to favourites")
                } else if channel.favorite {
                    Image(systemName: "heart.fill").foregroundStyle(theme.accent)
                        .accessibilityLabel("Favourite")
                }
                if let addToNewList {
                    Button(action: addToNewList) {
                        Image(systemName: "plus").font(.ui(size: 14))
                            .foregroundStyle(theme.textFaint)
                            .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Add \(channel.name) to a new list")
                }
                if !compact {
                Image(systemName: "play.fill")
                    .font(.ui(size: Daylight.px(14)))
                    .frame(width: Daylight.px(36), height: Daylight.px(36))
                    .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: Daylight.px(8), style: .continuous))
                    .accessibilityHidden(true)
                }
            }
        }
        .padding(.horizontal, compact ? 12 : 16)
        .frame(minHeight: Daylight.px(68))
        .foregroundStyle(theme.text)
    }
}

/// Windows' channel card: the logo with its badges and number, the name, what is on,
/// how far through, what is next, and the heart in the corner.
private struct LiveChannelCard: View {
    let channel: Channel
    let guide: NowNext?
    var toggleFavorite: (() -> Void)?
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(alignment: .top) {
                ChannelTile(name: channel.name, number: channel.number, logo: channel.logo, size: Daylight.px(52))
                Spacer()
                HStack(spacing: 4) {
                    if channel.stale {
                        chip("Dropped", fill: .orange, ink: .black)
                    }
                    if channel.hasCatchup { chip("Catch-up", fill: .green, ink: .white) }
                    if let quality = channel.quality { chip(quality, fill: theme.surfaceSunk, ink: theme.text) }
                    if channel.logo != nil, let number = channel.number {
                        Text(String(number)).fontWeight(.heavy).foregroundStyle(theme.textFaint)
                    }
                }
                .font(.ui(size: Daylight.px(12), weight: .heavy))
                .padding(.top, Daylight.px(14))
            }
            .padding(.bottom, 8)
            Text(channel.name).font(.ui(size: Daylight.px(17), weight: .heavy)).lineLimit(1)
                .padding(.trailing, 30)
            Text(guide?.now?.title ?? "No guide data")
                .font(.ui(size: Daylight.px(13)))
                .foregroundStyle(theme.textMuted)
                .lineLimit(1)
                .padding(.bottom, 8)
            DaylightProgress(value: programmeProgress(guide?.now), height: 6)
            HStack {
                Text(guide?.next.map { "Next · \($0.title)" } ?? " ")
                    .font(.ui(size: Daylight.px(13), weight: .bold))
                    .foregroundStyle(theme.textMuted)
                    .lineLimit(1)
                Spacer(minLength: 4)
                if let toggleFavorite {
                    Button(action: toggleFavorite) {
                        Image(systemName: channel.favorite ? "heart.fill" : "heart")
                            .foregroundStyle(channel.favorite ? theme.accent : theme.textFaint)
                            .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(channel.favorite ? "Remove \(channel.name) from favourites"
                                                         : "Add \(channel.name) to favourites")
                } else if channel.favorite {
                    Image(systemName: "heart.fill").foregroundStyle(theme.accent)
                }
            }
            .padding(.top, 6)
        }
        .padding(14)
        .frame(maxWidth: .infinity, minHeight: Daylight.px(176), alignment: .topLeading)
        .foregroundStyle(theme.text)
    }

    private func chip(_ text: String, fill: Color, ink: Color) -> some View {
        Text(text)
            .lineLimit(1)
            .padding(.horizontal, 10)
            .frame(height: Daylight.px(24))
            .foregroundStyle(ink)
            .background(fill, in: Capsule())
    }
}

/// Windows' `Badge`, in the tones the channel row uses.
struct Badge: View {
    enum Style { case neutral, new, warning, live, outline, accent }
    let text: String
    let style: Style
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        Text(text)
            .font(.ui(.caption2).weight(.heavy))
            .lineLimit(1).fixedSize()
            .padding(.horizontal, 8).padding(.vertical, 3)
            .foregroundStyle(foreground)
            .background(background, in: Capsule())
            .textCase(style == .live ? .uppercase : nil)
            .overlay {
                if style == .warning { Capsule().strokeBorder(.orange) }
                if style == .outline { Capsule().strokeBorder(.secondary, lineWidth: 1.5) }
            }
    }

    private var foreground: Color {
        switch style {
        case .neutral: .primary
        case .new, .live: .white
        case .warning: .orange
        case .outline: .secondary
        case .accent: theme.accentText
        }
    }

    private var background: Color {
        switch style {
        case .neutral: Color.secondary.opacity(0.18)
        case .new: .green
        case .live: .red
        case .accent: theme.accent
        case .warning, .outline: .clear
        }
    }
}
