import SwiftUI

/// Groups, then the channels in one, then the player. The spike's whole interface.
struct LiveView: View {
    @Environment(AppModel.self) private var model
    @State private var addingProvider = false
    /// Favourites first, then the profile's named lists (Windows' sidebar entries).
    @State private var lists: [FavoriteList] = []

    var body: some View {
        NavigationStack {
            List {
                PageHeading("Live TV")
                #if os(tvOS)
                // A row rather than the toolbar, which cuts the label to "Add…vider".
                Button("Add provider", systemImage: "plus") { addingProvider = true }
                #endif
                if let progress = model.importProgress {
                    ImportProgressRow(progress: progress)
                }
                if let report = model.lastImport {
                    Text("Imported \(report.channels) channels, \(report.movies) films and \(report.series) series.")
                        .foregroundStyle(.secondary)
                }
                if model.providers.isEmpty && model.importProgress == nil {
                    ContentUnavailableView(
                        "No providers yet",
                        systemImage: "antenna.radiowaves.left.and.right",
                        description: Text("Add an Xtream Codes panel or a playlist to start watching.")
                    )
                }
                Section {
                    ForEach(lists, id: \.name) { list in
                        let named = list.name == defaultFavorites ? nil : list.name
                        NavigationLink {
                            ChannelListView(group: nil, favoritesOnly: true, list: named)
                        } label: {
                            LabeledContent {
                                Text("\(list.channels)")
                            } label: {
                                Label(named ?? "Favourites", systemImage: "heart")
                            }
                        }
                    }
                    NavigationLink("All channels") { ChannelListView(group: nil) }
                    ForEach(model.groups, id: \.name) { group in
                        NavigationLink {
                            ChannelListView(group: group.name)
                        } label: {
                            LabeledContent(group.name, value: "\(group.count)")
                        }
                    }
                }
            }
            .auroraTitle("Live TV")
            .toolbar {
                #if !os(tvOS)
                Button("Add provider", systemImage: "plus") { addingProvider = true }
                #endif
            }
            .sheet(isPresented: $addingProvider) {
                SetupWizard(additional: true, onFinished: { addingProvider = false }, onSkip: { addingProvider = false })
                    .auroraAppearance()
            }
            .task { await model.reload() }
            // Every time the screen comes back: a list made or emptied in a channel list
            // shows here at once.
            .task(id: model.profileId) { lists = await model.favoriteLists() }
            .onAppear { Task { lists = await model.favoriteLists() } }
        }
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
                ProgressView(value: Double(progress.done), total: Double(progress.total))
            } else {
                ProgressView()
            }
        }
    }
}

struct ChannelListView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let group: String?
    var favoritesOnly = false
    /// Which favourites list: the heart means "in this list" wherever it is pressed, as on
    /// Windows. Nil is the default Favourites.
    var list: String?
    @State private var channels: [Channel] = []
    /// The first read finished: until then empty means "not read yet".
    @State private var loaded = false
    @State private var guide: [Int64: NowNext] = [:]
    @State private var playing: PlayerItem?
    @State private var guideFor: Channel?
    /// "Add to a new list…" is asking for a name for this channel.
    @State private var newListFor: Channel?
    @State private var newListName = ""
    @State private var renaming = false
    @State private var renameTo = ""
    @State private var confirmingDelete = false

    private var listTitle: String { list ?? "Favourites" }

    var body: some View {
        List(channels, id: \.id) { channel in
            Button {
                playing = .channel(id: channel.id, name: channel.name)
            } label: {
                ChannelRow(channel: channel, guide: guide[channel.id],
                           locked: model.isLocked(channelId: channel.id, group: channel.group),
                           toggleFavorite: heart(channel))
            }
            // Long-press on the Siri Remote, long-press or right-click on iPhone and iPad.
            .contextMenu { menu(for: channel) }
        }
        .overlay {
            if favoritesOnly && channels.isEmpty && loaded {
                // Windows' words, with its "heart beside any channel" where there is one.
                ContentUnavailableView(list == nil ? "No favourite channels yet" : "\(listTitle) is empty",
                                       systemImage: "heart",
                                       description: Text(heartHint))
            } else if channels.isEmpty && loaded {
                ContentUnavailableView("No channels", systemImage: "tv",
                                       description: Text(emptyReason))
            }
        }
        .auroraPageTitle(favoritesOnly ? listTitle : group ?? "All channels")
        .toolbar {
            // A named list can be renamed or deleted; the default is always there.
            if favoritesOnly, list != nil {
                Menu("List", systemImage: "ellipsis.circle") {
                    Button("Rename list", systemImage: "pencil") { renameTo = listTitle; renaming = true }
                    Button("Delete list", systemImage: "trash", role: .destructive) { confirmingDelete = true }
                }
            }
        }
        .task { await load() }
        // The padlocks: what a parent locked, read again each visit (Windows' `LivePage`).
        .task { await model.loadParental() }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
        .navigationDestination(item: $guideFor) { ChannelGuideView(channel: $0) }
        .alert("Add to a new list", isPresented: Binding(get: { newListFor != nil }, set: { if !$0 { newListFor = nil } })) {
            TextField("List name", text: $newListName)
            Button("Add") {
                // Read now: closing the alert clears `newListFor` before a task would run.
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
        if favoritesOnly, let i = channels.firstIndex(where: { $0.id == channel.id }) {
            if i > 0 {
                Button("Move up", systemImage: "arrow.up") { Task { await move(from: i, to: i - 1) } }
            }
            if i < channels.count - 1 {
                Button("Move down", systemImage: "arrow.down") { Task { await move(from: i, to: i + 1) } }
            }
        }
    }

    /// The heart beside each row, where a row has room for a second thing to press.
    private func heart(_ channel: Channel) -> (() -> Void)? {
        #if os(tvOS)
        nil
        #else
        { Task { await toggle(channel) } }
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

    private func load() async {
        channels = await model.channels(group: group, favoritesOnly: favoritesOnly, list: list)
        loaded = true
        let answer = await model.nowNext(channelIds: channels.map(\.id))
        guide = Dictionary(answer.map { ($0.channelId, $0) }, uniquingKeysWith: { a, _ in a })
    }

    private func toggle(_ channel: Channel) async {
        guard let now = await model.toggleFavorite(channelId: channel.id, list: list) else { return }
        if favoritesOnly && !now {
            channels.removeAll { $0.id == channel.id }
        } else if let i = channels.firstIndex(where: { $0.id == channel.id }) {
            channels[i].favorite = now
        }
    }

    /// A list exists once something is in it, so making one and filling it are the same
    /// step, as on Windows.
    private func addToNewList(_ channel: Channel, name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != list else { return }
        _ = await model.toggleFavorite(channelId: channel.id, list: name)
    }

    private func move(from: Int, to: Int) async {
        var next = channels
        next.insert(next.remove(at: from), at: to)
        channels = next
        await model.reorderFavorites(list: list, channelIds: next.map(\.id))
    }

    private func rename(to name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let from = list, !name.isEmpty, name != from else { return }
        await model.renameFavoriteList(from, to: name)
        dismiss() // back to Live TV, which lists it under its new name
    }

    private func deleteList() async {
        guard let name = list else { return }
        await model.deleteFavoriteList(name)
        dismiss()
    }
}

/// Windows' channel row (`LivePage`): the number, the logo, the name with its padlock and
/// quality, what is on (its start time and title, or "No guide data") and what is next,
/// how far through, then the Dropped and Catch-up badges and the heart.
private struct ChannelRow: View {
    let channel: Channel
    let guide: NowNext?
    /// Locked by a parent: playing it asks for the parental PIN.
    var locked = false
    /// The heart, where it can be pressed apart from the row (iPhone and iPad). A TV row
    /// is one thing to focus, so there the heart only shows; the long press changes it.
    var toggleFavorite: (() -> Void)?
    /// tvOS: the focused row turns white, and a white bar would vanish on it.
    @Environment(\.isFocused) private var focused

    var body: some View {
        HStack(spacing: 14) {
            Text(channel.number.map(String.init) ?? "")
                .font(.body.weight(.heavy)).monospacedDigit()
                .foregroundStyle(.tertiary)
                // A provider's numbers run to five digits.
                .lineLimit(1).minimumScaleFactor(0.6)
                .frame(width: numberWidth, alignment: .trailing)
            ChannelTile(name: channel.name, number: channel.number, logo: channel.logo, size: logoSize)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(channel.name).font(.headline.weight(.heavy)).lineLimit(1)
                    if locked {
                        Image(systemName: "lock.fill").font(.caption).foregroundStyle(.secondary)
                            .accessibilityLabel("Locked: needs the parental PIN")
                    }
                    if let quality = channel.quality {
                        Badge(text: quality, style: .neutral)
                    }
                }
                (Text(guide?.now.map { "\(clockTime(unix: $0.start)) \($0.title)" } ?? "No guide data")
                 + Text(guide?.next.map { "  ·  Next: \($0.title)" } ?? "").foregroundStyle(.tertiary))
                    .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                if let now = guide?.now, elapsed(now) > 0 {
                    ProgressView(value: elapsed(now))
                        #if os(tvOS)
                        .tint(focused ? .black : nil)
                        #endif
                        .frame(maxWidth: 260)
                        .padding(.top, 4)
                        .accessibilityLabel("Programme progress")
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 6) {
                if channel.stale {
                    Badge(text: "Dropped", style: .warning)
                        .accessibilityHint("Your provider no longer lists this channel")
                }
                if channel.hasCatchup { Badge(text: "Catch-up", style: .new) }
                if let toggleFavorite {
                    Button(action: toggleFavorite) {
                        Image(systemName: channel.favorite ? "heart.fill" : "heart")
                            .foregroundStyle(channel.favorite ? Color.accentColor : Color.secondary)
                            .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(channel.favorite ? "Remove \(channel.name) from favourites"
                                                         : "Add \(channel.name) to favourites")
                } else if channel.favorite {
                    Image(systemName: "heart.fill").foregroundStyle(Color.accentColor)
                        .accessibilityLabel("Favourite")
                }
            }
        }
        // A list tints a button's label; the row is text, not a link. Not on tvOS, where
        // the focused row turns white and its text must turn dark with it.
        #if os(iOS)
        .foregroundStyle(Color.primary)
        #endif
    }

    #if os(tvOS)
    private let numberWidth: CGFloat = 80
    private let logoSize: CGFloat = 72
    #else
    private let numberWidth: CGFloat = 40
    private let logoSize: CGFloat = 44
    #endif

    private func elapsed(_ p: Programme) -> Double {
        let now = Date().timeIntervalSince1970
        let span = Double(p.stop - p.start)
        guard span > 0 else { return 0 }
        return min(max((now - Double(p.start)) / span, 0), 1)
    }
}

/// Windows' `Badge`, in the tones the channel row uses.
struct Badge: View {
    enum Style { case neutral, new, warning, live, outline }
    let text: String
    let style: Style

    var body: some View {
        Text(text)
            .font(.caption2.weight(.heavy))
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
        }
    }

    private var background: Color {
        switch style {
        case .neutral: Color.secondary.opacity(0.18)
        case .new: .green
        case .live: .red
        case .warning, .outline: .clear
        }
    }
}
