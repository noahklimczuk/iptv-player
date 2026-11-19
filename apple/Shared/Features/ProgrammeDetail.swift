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
    /// The channel's other copies, by quality (Windows' `ChannelSources`).
    @State private var sources: [Alternate] = []
    @Environment(\.searchFor) private var searchFor

    var body: some View {
        #if os(tvOS)
        // A page of its own, as the TV app shows a programme: a sheet on a TV is a narrow
        // card that cut the buttons off. Menu closes it, so there is no Done.
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(channel.name).font(.headline).foregroundStyle(.secondary)
                details
                actions.padding(.top, 12)
            }
            .padding(80)
            .frame(maxWidth: 1400, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(.regularMaterial)
        .task { await model.loadDvrMarks() }
        .task { await loadSources() }
        #else
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    details
                    actions.padding(.top, 8)
                }
                .padding()
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .auroraPageTitle(channel.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { await model.loadDvrMarks() }
            .task { await loadSources() }
        }
        .presentationDetents([.medium, .large])
        #endif
    }

    /// What it is and when, above the buttons.
    @ViewBuilder private var details: some View {
        badges
        Text(p.title).font(.title.weight(.heavy))
        if let sub = p.subTitle {
            Text(sub).foregroundStyle(.secondary)
        }
        Text(timing).font(.callout).monospacedDigit().foregroundStyle(.tertiary)
        if let description = p.description {
            Text(description).font(.callout).foregroundStyle(.secondary)
        }
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
        // A row on a TV, where the remote moves sideways between them; a column on a
        // phone, where a row of four would not fit.
        #if os(tvOS)
        let layout = AnyLayout(HStackLayout(spacing: 24))
        #else
        let layout = AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
        #endif
        layout {
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
        }
        .buttonStyle(.bordered)
        // Windows' order: the channel's other copies, then the search, under the rest.
        if !sources.isEmpty {
            HStack(spacing: 6) {
                Text("Also in").font(.caption).foregroundStyle(.tertiary)
                ForEach(sources, id: \.id) { source in
                    Button(source.quality ?? "Unknown") {
                        play(.channel(id: source.id, name: source.name))
                    }
                    .buttonStyle(.borderless)
                }
            }
        }
        Button("Search this title", systemImage: "magnifyingglass") {
            let title = p.title
            dismiss()
            // Once this has gone: Search may be a sheet of its own on a phone, and one
            // presentation cannot start while another is still leaving.
            Task {
                try? await Task.sleep(for: .milliseconds(450))
                searchFor(title)
            }
        }
        .buttonStyle(.borderless)
        if let failure {
            Text(failure).font(.callout).foregroundStyle(.red)
        }
    }

    private func loadSources() async {
        sources = await model.alternates(.channels, id: channel.id).filter { $0.id != channel.id }
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

extension View {
    /// Show `ProgrammeDetail`: a sheet on iPhone and iPad, a page of its own on a TV.
    func programmeDetail<Content: View>(
        item: Binding<ChosenProgramme?>,
        onDismiss: (() -> Void)? = nil,
        @ViewBuilder content: @escaping (ChosenProgramme) -> Content
    ) -> some View {
        // The theme too: a presentation keeps the system's appearance otherwise.
        #if os(tvOS)
        fullScreenCover(item: item, onDismiss: onDismiss) { content($0).auroraAppearance() }
        #else
        sheet(item: item, onDismiss: onDismiss) { content($0).auroraAppearance() }
        #endif
    }
}

/// A programme chosen in a guide, for the detail sheet.
struct ChosenProgramme: Identifiable {
    let channel: Channel
    let programme: Programme
    let replayable: Bool
    var id: String { "\(channel.id)-\(programme.id)" }
}
