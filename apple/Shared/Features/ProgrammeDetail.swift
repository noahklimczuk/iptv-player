import SwiftUI

/// One programme chosen in the guide: Windows' `InfoPane` (`GuidePage.tsx`), as a sheet.
///
/// What it is, when, and what can be done with it: watch it live or from the archive,
/// record it, record the series, or be reminded. The buttons and their words are
/// Windows'; the marks come from `AppModel.dvrMarks`, Windows' `useDvrMarks`.
struct ProgrammeDetail: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let channel: Channel
    let programme: Programme
    /// Already aired and still in the channel's archive.
    let replayable: Bool
    /// Close the sheet and play this.
    let play: (PlayerItem) -> Void

    @State private var busy = false
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    badges
                    Text(p.title).font(.title.weight(.heavy))
                    if let sub = p.subTitle {
                        Text(sub).foregroundStyle(.secondary)
                    }
                    Text(timing).font(.callout).monospacedDigit().foregroundStyle(.tertiary)
                    if let description = p.description {
                        Text(description).font(.callout).foregroundStyle(.secondary)
                    }
                    actions.padding(.top, 8)
                }
                .padding()
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .navigationTitle(channel.name)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { await model.loadDvrMarks() }
        }
        .presentationDetents([.medium, .large])
    }

    private var p: Programme { programme }
    private var now: Int64 { Int64(Date().timeIntervalSince1970) }
    private var airing: Bool { p.start <= now && p.stop > now }
    /// Nothing to schedule for a programme that is over; catch-up is another button.
    private var ended: Bool { p.stop <= now }

    private var timing: String {
        var text = "\(Format.clock(p.start))–\(Format.clock(p.stop)) · \(Format.duration(p.stop - p.start))"
        if let s = p.season, let e = p.episode, s > 0, e > 0 { text += " · S\(s)E\(e)" }
        return text
    }

    private var badges: some View {
        HStack(spacing: 6) {
            if p.isNew { badge("New", .green) }
            if p.isLive { badge("Live", .red) }
            if p.isPremiere { badge("Premiere", .accentColor) }
            if let rating = p.rating { badge(rating, nil) }
            if let quality = channel.quality { badge(quality, .gray) }
        }
    }

    private func badge(_ text: String, _ tint: Color?) -> some View {
        Text(text).font(.caption.bold()).padding(.horizontal, 8).padding(.vertical, 3)
            .background(tint ?? .clear, in: Capsule())
            .overlay { if tint == nil { Capsule().strokeBorder(.secondary) } }
            .foregroundStyle(tint == nil ? Color.primary : .white)
    }

    @ViewBuilder private var actions: some View {
        let marks = model.dvrMarks
        let recording = marks.recording(channelId: channel.id, p)
        let reminder = marks.reminder(channelId: channel.id, p)
        let rule = marks.rule(p)
        VStack(alignment: .leading, spacing: 8) {
            if airing {
                Button("Watch now", systemImage: "play.fill") {
                    play(.channel(id: channel.id, name: channel.name))
                }
                .buttonStyle(.borderedProminent)
            }
            // Anything already started, not only what is on now: the point of catch-up
            // is the programme that finished an hour ago.
            if replayable {
                Button(airing ? "Watch from start" : "Watch this", systemImage: "gobackward.10") {
                    play(.catchup(channelId: channel.id, title: p.title, start: p.start, stop: p.stop))
                }
            }
            Button(recording.map { $0.state == .recording ? "Stop recording" : "Cancel recording" } ?? "Record",
                   systemImage: recording != nil ? "record.circle.fill" : "record.circle") {
                run { await model.toggleRecord(channelId: channel.id, p) }
            }
            .tint(recording != nil ? .red : nil)
            .disabled(busy || ended)
            Button(rule != nil ? "Stop recording series" : "Record series", systemImage: "square.stack") {
                run { await model.toggleSeries(channelId: channel.id, p, anyChannel: false) }
            }
            .tint(rule != nil ? .accentColor : nil)
            .disabled(busy)
            if !airing && !ended {
                Button(reminder != nil ? "Reminder set" : "Remind me",
                       systemImage: reminder != nil ? "bell.fill" : "bell") {
                    run { await model.toggleReminder(channelId: channel.id, p) }
                }
                .tint(reminder != nil ? .accentColor : nil)
                .disabled(busy)
            }
            if let failure {
                Text(failure).font(.callout).foregroundStyle(.red)
            }
        }
        .buttonStyle(.bordered)
    }

    private func run(_ body: @escaping () async -> String?) {
        busy = true
        failure = nil
        Task {
            failure = await body()
            busy = false
        }
    }
}

/// A programme chosen in a guide, for the detail sheet.
struct ChosenProgramme: Identifiable {
    let channel: Channel
    let programme: Programme
    let replayable: Bool
    var id: String { "\(channel.id)-\(programme.id)" }
}
