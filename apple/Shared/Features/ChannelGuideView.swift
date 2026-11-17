import SwiftUI

/// One channel's schedule, a day at a time. What is on now plays live; what already aired
/// replays from the provider's archive where the channel keeps one (catch-up); what is
/// still to come is listed.
struct ChannelGuideView: View {
    @Environment(AppModel.self) private var model
    let channel: Channel

    /// 0 is today, -1 yesterday. Channels with an archive can look back a week.
    @State private var day = 0
    @State private var programmes: [Programme] = []
    @State private var replayable: Set<Int64> = []
    @State private var loading = true
    @State private var playing: PlayerItem?

    private var days: [Int] { channel.hasCatchup ? Array(-7...1) : [0, 1] }

    var body: some View {
        List {
            Picker("Day", selection: $day) {
                ForEach(days, id: \.self) { Text(Self.dayName($0)).tag($0) }
            }
            ForEach(programmes, id: \.id) { programme in
                row(programme)
            }
        }
        .overlay {
            if !loading && programmes.isEmpty {
                ContentUnavailableView("No guide for this day", systemImage: "calendar",
                                       description: Text("The provider's guide doesn't cover it."))
            }
        }
        .navigationTitle(channel.name)
        .task(id: day) { await load() }
        .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
    }

    @ViewBuilder
    private func row(_ p: Programme) -> some View {
        let now = Int64(Date().timeIntervalSince1970)
        let onNow = p.start <= now && now < p.stop
        if onNow {
            Button { playing = .channel(id: channel.id, name: channel.name) } label: {
                ProgrammeLabel(programme: p, onNow: true, mark: Text("Live").bold())
            }
        } else if replayable.contains(p.id) {
            Button {
                playing = .catchup(channelId: channel.id, title: p.title, start: p.start, stop: p.stop)
            } label: {
                // `.badge` would be the list idiom, but tvOS doesn't have it.
                ProgrammeLabel(programme: p, onNow: false, mark: Text(Image(systemName: "gobackward")))
            }
            .accessibilityHint("Plays from the channel's archive")
        } else {
            ProgrammeLabel(programme: p, onNow: false, mark: nil)
                .foregroundStyle(p.stop <= now ? .secondary : .primary)
        }
    }

    private func load() async {
        loading = true
        let calendar = Calendar.current
        let start = calendar.date(byAdding: .day, value: day, to: calendar.startOfDay(for: Date()))!
        let end = calendar.date(byAdding: .day, value: 1, to: start)!
        let list = await model.schedule(channelId: channel.id, from: start, to: end)
        programmes = list
        replayable = channel.hasCatchup ? await model.replayable(channelId: channel.id, list) : []
        loading = false
    }

    private static func dayName(_ offset: Int) -> String {
        switch offset {
        case 0: return "Today"
        case -1: return "Yesterday"
        case 1: return "Tomorrow"
        default:
            let date = Calendar.current.date(byAdding: .day, value: offset, to: Date())!
            return date.formatted(.dateTime.weekday(.wide).day().month())
        }
    }
}

private struct ProgrammeLabel: View {
    let programme: Programme
    let onNow: Bool
    /// "Live", or the replay mark, at the end of the row.
    let mark: Text?

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(Date(timeIntervalSince1970: TimeInterval(programme.start)), format: .dateTime.hour().minute())
                .monospacedDigit()
                .foregroundStyle(onNow ? .primary : .secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(programme.title).fontWeight(onNow ? .semibold : .regular)
                if let sub = programme.subTitle {
                    Text(sub).font(.caption).foregroundStyle(.secondary)
                }
                if let description = programme.description {
                    Text(description).font(.caption2).foregroundStyle(.tertiary).lineLimit(2)
                }
            }
            Spacer(minLength: 0)
            if let mark { mark.foregroundStyle(.secondary) }
        }
    }
}
