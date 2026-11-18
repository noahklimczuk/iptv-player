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
            .navigationTitle("Live TV")
            .toolbar {
                Button("Add provider", systemImage: "plus") { addingProvider = true }
            }
            .sheet(isPresented: $addingProvider) {
                SetupWizard(additional: true, onFinished: { addingProvider = false }, onSkip: { addingProvider = false })
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
                ChannelRow(channel: channel, guide: guide[channel.id])
            }
            // Long-press on the Siri Remote, long-press or right-click on iPhone and iPad.
            .contextMenu { menu(for: channel) }
        }
        .overlay {
            if favoritesOnly && channels.isEmpty {
                ContentUnavailableView(list == nil ? "No favourites yet" : "\(listTitle) is empty",
                                       systemImage: "heart",
                                       description: Text("Long-press a channel to add it."))
            }
        }
        .navigationTitle(favoritesOnly ? listTitle : group ?? "All channels")
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

    private func load() async {
        channels = await model.channels(group: group, favoritesOnly: favoritesOnly, list: list)
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

private struct ChannelRow: View {
    let channel: Channel
    let guide: NowNext?

    var body: some View {
        HStack(spacing: 12) {
            if let number = channel.number {
                Text("\(number)").monospacedDigit().foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(channel.name)
                    if channel.favorite {
                        Image(systemName: "heart.fill").font(.caption).foregroundStyle(.pink)
                            .accessibilityLabel("Favourite")
                    }
                    if channel.hasCatchup {
                        Image(systemName: "gobackward").font(.caption).foregroundStyle(.secondary)
                            .accessibilityLabel("Catch-up")
                    }
                    if channel.stale {
                        Text("Dropped").font(.caption).foregroundStyle(.orange)
                    }
                }
                if let now = guide?.now {
                    Text("Now: \(now.title)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    ProgressView(value: elapsed(now)).tint(.secondary)
                }
                if let next = guide?.next {
                    Text("Next: \(next.title)").font(.caption2).foregroundStyle(.tertiary).lineLimit(1)
                }
            }
        }
    }

    private func elapsed(_ p: Programme) -> Double {
        let now = Date().timeIntervalSince1970
        let span = Double(p.stop - p.start)
        guard span > 0 else { return 0 }
        return min(max((now - Double(p.start)) / span, 0), 1)
    }
}
