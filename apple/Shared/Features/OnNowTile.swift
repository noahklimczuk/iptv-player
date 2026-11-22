import SwiftUI

/// Home's "On now" tile (Windows' `OnNowTile`): the viewer's favourite channels and what
/// they are airing, from the same two questions Live TV asks (the favourites, then now
/// and next for them). Six, because a tile is a glance and not a guide.
struct OnNowTile: View {
    @Environment(AppModel.self) private var model
    @State private var channels: [Channel]?
    @State private var guide: [Int64: Programme] = [:]
    @State private var playing: PlayerItem?
    /// Changes when the list could have: the library, the profile, or back from playing.
    let reload: AnyHashable

    static let shown: UInt32 = 6

    var body: some View {
        VStack(alignment: .leading, spacing: rowSpacing) {
            HStack {
                Text("On now").displayFont(.title2)
                Spacer()
                Label {
                    Text("LIVE").font(.ui(.caption).weight(.heavy))
                } icon: {
                    Circle().frame(width: 7, height: 7)
                }
                .padding(.horizontal, 10).padding(.vertical, 4)
                .foregroundStyle(.red)
                .background(.red.opacity(0.14), in: Capsule())
                .accessibilityLabel("Live")
            }
            if let channels, channels.isEmpty {
                Text("Heart a channel in Live TV and it will show up here with what it is airing.")
                    .font(.ui(.subheadline))
                    .foregroundStyle(.secondary)
            }
            // Progress is about the clock, so it has to move on its own (every 30 s).
            Ticking(every: 30) { context in
                VStack(spacing: rowSpacing) {
                    ForEach(channels ?? [], id: \.id) { channel in
                        row(channel, now: context.date)
                    }
                }
            }
            #if os(tvOS)
            .focusSection()
            #endif
        }
        // As tall as the billboard beside it.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(EdgeInsets(top: 24, leading: 24, bottom: 20, trailing: 24))
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 24))
        .task(id: [reload, AnyHashable(playing == nil)]) { if playing == nil { await load() } }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
    }

    private func row(_ channel: Channel, now: Date) -> some View {
        let on = guide[channel.id]
        return Button { playing = .channel(id: channel.id, name: channel.name) } label: {
            HStack(spacing: 14) {
                ChannelTile(name: channel.name, number: channel.number, logo: channel.logo, size: tileSize)
                VStack(alignment: .leading, spacing: 2) {
                    Text(channel.name).font(.ui(.headline).weight(.heavy)).lineLimit(1)
                    Text(on?.title ?? "No guide").font(.ui(.subheadline)).foregroundStyle(.secondary).lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                VStack(alignment: .trailing, spacing: 8) {
                    Text(on.map { "\(time($0.start))–\(time($0.stop))" } ?? " ")
                        .font(.ui(.caption).weight(.bold)).monospacedDigit().foregroundStyle(.secondary)
                    ProgressView(value: on.map { through($0, now) } ?? 0)
                }
                .frame(maxWidth: .infinity)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(on.map { "Watch \(channel.name), \($0.title)" } ?? "Watch \(channel.name)")
    }

    private func load() async {
        let favourites = await model.channels(group: nil, favoritesOnly: true, limit: Self.shown)
        let answer = await model.nowNext(channelIds: favourites.map(\.id))
        guide = Dictionary(answer.compactMap { n in n.now.map { (n.channelId, $0) } },
                           uniquingKeysWith: { a, _ in a })
        channels = favourites
    }

    private func through(_ p: Programme, _ now: Date) -> Double {
        min(1, max(0, (now.timeIntervalSince1970 - Double(p.start)) / Double(max(1, p.stop - p.start))))
    }

    private func time(_ unix: Int64) -> String {
        clockTime(unix: unix)
    }

    #if os(tvOS)
    private let tileSize: CGFloat = 72
    private let rowSpacing: CGFloat = 18
    #else
    private let tileSize: CGFloat = 48
    private let rowSpacing: CGFloat = 12
    #endif
}
