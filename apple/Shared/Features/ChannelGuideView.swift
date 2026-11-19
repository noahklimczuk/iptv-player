import SwiftUI

/// One channel's schedule, a day at a time. Choosing a programme opens its details, as
/// the guide's do: what is on now plays live, what already aired replays from the
/// provider's archive where the channel keeps one (catch-up), and anything still to come
/// can be recorded or remembered.
struct ChannelGuideView: View {
    @Environment(AppModel.self) private var model
    let channel: Channel

    /// 0 is today, -1 yesterday. Channels with an archive can look back a week.
    @State private var day = 0
    @State private var programmes: [Programme] = []
    @State private var replayable: Set<Int64> = []
    @State private var loading = true
    @State private var playing: PlayerItem?
    @State private var chosen: ChosenProgramme?
    @State private var afterSheet: PlayerItem?

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
        .sheet(item: $chosen, onDismiss: {
            if let item = afterSheet { afterSheet = nil; playing = item }
        }) { c in
            ProgrammeDetail(channel: c.channel, programme: c.programme, replayable: c.replayable) { item in
                afterSheet = item
                chosen = nil
            }
        }
        .task(id: model.recordingsVersion) { await model.loadDvrMarks() }
    }

    private func row(_ p: Programme) -> some View {
        let now = Int64(Date().timeIntervalSince1970)
        let onNow = p.start <= now && now < p.stop
        let canReplay = replayable.contains(p.id)
        let recording = model.dvrMarks.recording(channelId: channel.id, p) != nil
        return Button {
            chosen = ChosenProgramme(channel: channel, programme: p, replayable: canReplay)
        } label: {
            // `.badge` would be the list idiom, but tvOS doesn't have it.
            ProgrammeLabel(programme: p, onNow: onNow, recording: recording,
                           mark: onNow ? Text("Live").bold()
                               : canReplay ? Text(Image(systemName: "gobackward")) : nil)
                .foregroundStyle(p.stop <= now && !canReplay ? .secondary : .primary)
        }
        .accessibilityHint(canReplay ? "Plays from the channel's archive" : "")
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
    /// A recording is scheduled for it (the guide's red dot).
    var recording = false
    /// "Live", or the replay mark, at the end of the row.
    let mark: Text?

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(Date(timeIntervalSince1970: TimeInterval(programme.start)), format: .dateTime.hour().minute())
                .monospacedDigit()
                .foregroundStyle(onNow ? .primary : .secondary)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 5) {
                    if recording {
                        Circle().fill(.red).frame(width: 7, height: 7)
                            .accessibilityLabel("Recording scheduled")
                    }
                    Text(programme.title).fontWeight(onNow ? .semibold : .regular)
                }
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
