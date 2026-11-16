import SwiftUI

/// Groups, then the channels in one, then the player. The spike's whole interface.
struct LiveView: View {
    @Environment(AppModel.self) private var model
    @State private var addingProvider = false

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
                    NavigationLink { ChannelListView(group: nil, favoritesOnly: true) } label: {
                        Label("Favourites", systemImage: "heart")
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
                NavigationStack { AddProviderView() }
            }
            .alert("Something went wrong", isPresented: errorShown) {
                Button("OK") { model.error = nil }
            } message: {
                Text(model.error ?? "")
            }
            .task { await model.reload() }
        }
    }

    private var errorShown: Binding<Bool> {
        Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })
    }
}

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
    let group: String?
    var favoritesOnly = false
    @State private var channels: [Channel] = []
    @State private var guide: [Int64: NowNext] = [:]
    @State private var playing: PlayerItem?

    var body: some View {
        List(channels, id: \.id) { channel in
            Button {
                playing = .channel(id: channel.id, name: channel.name)
            } label: {
                ChannelRow(channel: channel, guide: guide[channel.id])
            }
            // Long-press on the Siri Remote, long-press or right-click on iPhone and iPad.
            .contextMenu {
                Button(channel.favorite ? "Remove from Favourites" : "Add to Favourites",
                       systemImage: channel.favorite ? "heart.slash" : "heart") {
                    Task { await toggle(channel) }
                }
            }
        }
        .overlay {
            if favoritesOnly && channels.isEmpty {
                ContentUnavailableView("No favourites yet", systemImage: "heart",
                                       description: Text("Long-press a channel to add it."))
            }
        }
        .navigationTitle(favoritesOnly ? "Favourites" : group ?? "All channels")
        .task {
            channels = await model.channels(group: group, favoritesOnly: favoritesOnly)
            let answer = await model.nowNext(channelIds: channels.map(\.id))
            guide = Dictionary(answer.map { ($0.channelId, $0) }, uniquingKeysWith: { a, _ in a })
        }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
    }

    private func toggle(_ channel: Channel) async {
        guard let now = await model.toggleFavorite(channelId: channel.id) else { return }
        if favoritesOnly && !now {
            channels.removeAll { $0.id == channel.id }
        } else if let i = channels.firstIndex(where: { $0.id == channel.id }) {
            channels[i].favorite = now
        }
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
