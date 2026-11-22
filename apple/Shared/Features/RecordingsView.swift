import SwiftUI

/// The recordings library: Windows' `RecordingsPage` (README §7.7).
///
/// Three tabs rather than one list, because they answer different questions: "what can I
/// watch", "what is Aurora about to do", and "what went wrong". The words are Windows',
/// checked against `RecordingsPage.tsx` by `mirrors.rs`.
struct RecordingsView: View {
    @Environment(\.auroraTheme) private var theme
    @Environment(AppModel.self) private var model
    var closable = false
    @Environment(\.dismiss) private var dismiss

    enum RecordingsTab: Hashable { case recorded, scheduled, rules }

    @State private var tab: RecordingsTab = .recorded
    @State private var page: RecordingsPage?
    @State private var playing: PlayerItem?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    PageHeading("Recordings")
                    header
                    content
                }
                .padding(.horizontal, edge)
                .padding(.bottom, 24)
            }
            .auroraTitle("Recordings")
            .toolbar {
                if closable {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") { dismiss() }
                    }
                }
            }
            // A recording starting or finishing changes this page, and the scheduler
            // says when that happens, as Windows' `dvr.tick` does.
            .task(id: model.recordingsVersion) { page = await model.recordingsPage() }
            .fullScreenCover(item: $playing, onDismiss: { model.recordingsChanged() }) {
                PlayerScreen(item: $0)
            }
        }
    }

    private var recordings: [Recording] { page?.recordings ?? [] }
    private var recorded: [Recording] { recordings.filter { $0.state == .completed } }
    private var upcoming: [Recording] {
        recordings.filter { $0.state == .scheduled || $0.state == .recording }
    }
    private var problems: [Recording] {
        recordings.filter { $0.state == .failed || $0.state == .skipped }
    }

    private func count(_ t: RecordingsTab) -> Int {
        switch t {
        case .recorded: recorded.count
        case .scheduled: upcoming.count
        case .rules: page?.rules.count ?? 0
        }
    }

    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .center, spacing: 20) {
                tabs.fixedSize()
                Spacer()
                if let storage = page?.storage { StorageMeter(storage: storage) }
            }
            VStack(alignment: .leading, spacing: 12) {
                tabs
                if let storage = page?.storage { StorageMeter(storage: storage) }
            }
        }
    }

    /// Windows' `dl-seg` tablist, each tab with its count.
    private var tabs: some View {
        Segments(options: [
            (RecordingsTab.recorded, "Recorded \(count(.recorded))"),
            (RecordingsTab.scheduled, "Scheduled \(count(.scheduled))"),
            (RecordingsTab.rules, "Series rules \(count(.rules))"),
        ], selection: $tab)
        .accessibilityLabel("Recordings")
    }

    @ViewBuilder private var content: some View {
        if let page {
            if !page.conflicts.isEmpty && tab != .recorded {
                ConflictNotice(conflicts: page.conflicts, recordings: recordings)
            }
            switch tab {
            case .recorded:
                if recorded.isEmpty {
                    empty("Nothing recorded yet", "record.circle",
                          "Press Record on anything in the guide, or set up a series rule and Aurora will keep up with it for you.")
                } else {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: cardWidth), spacing: 16)], spacing: 16) {
                        ForEach(recorded, id: \.id) { rec in
                            RecordedCard(rec: rec) { play(rec) }
                        }
                    }
                }
                // Daylight's "Coming up" panel under the recorded grid.
                if !upcoming.isEmpty {
                    ComingUp(upcoming: Array(upcoming.prefix(5))) { tab = .rules }
                }
            case .scheduled:
                if upcoming.isEmpty {
                    empty("Nothing scheduled", "clock",
                          "Recordings you set from the guide show up here, with a countdown and anything that clashes.")
                } else {
                    VStack(spacing: 10) {
                        ForEach(upcoming, id: \.id) { ScheduledRow(rec: $0) }
                    }
                }
                if !problems.isEmpty {
                    Text("Didn’t record").displayFont(.title2).padding(.top, 24)
                    VStack(spacing: 10) {
                        ForEach(problems, id: \.id) { ProblemRow(rec: $0) }
                    }
                }
            case .rules:
                if page.rules.isEmpty {
                    empty("No series rules", "square.stack",
                          "A series rule records every episode of a show as it airs. Set one from the Record button in the guide.")
                } else {
                    VStack(spacing: 10) {
                        ForEach(page.rules, id: \.id) { RuleRow(rule: $0) }
                    }
                }
            }
        } else {
            VStack(spacing: 8) {
                ForEach(0..<6, id: \.self) { _ in
                    RoundedRectangle(cornerRadius: 18).fill(theme.surface).frame(height: 72)
                }
            }
        }
    }

    private func empty(_ title: String, _ icon: String, _ body: String) -> some View {
        ContentUnavailableView(title, systemImage: icon, description: Text(body))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 40)
    }

    /// Play from where it was left, unless it was finished or barely begun (Windows'
    /// `resumeAt`).
    private func play(_ rec: Recording) {
        Task {
            let saved = await model.progress(.recording, id: rec.id)
            let startAt = saved.flatMap { !$0.completed && $0.positionSecs > 60 ? Double($0.positionSecs) : nil }
            playing = .recording(id: rec.id, title: rec.title, startAt: startAt)
        }
    }

    #if os(tvOS)
    private let edge: CGFloat = 60
    private let cardWidth: CGFloat = 420
    #else
    private let edge: CGFloat = 16
    private let cardWidth: CGFloat = 300
    #endif
}

/// Mon … Sun, as a rule's weekdays are numbered (0 is Monday). Windows' `WEEKDAYS`.
private let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

// MARK: Storage

private struct StorageMeter: View {
    @Environment(\.auroraTheme) private var theme
    let storage: DvrStorage

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("Storage").foregroundStyle(theme.textMuted)
                Spacer(minLength: 12)
                Text(storage.quotaBytes > 0
                     ? "\(Format.bytes(storage.usedBytes)) of \(Format.bytes(storage.quotaBytes))"
                     : Format.bytes(storage.usedBytes))
                    .fontWeight(.bold)
            }
            .font(.ui(.callout).weight(.semibold))
            if storage.quotaBytes > 0 {
                ProgressView(value: min(1, Double(storage.usedBytes) / Double(storage.quotaBytes)))
                    .accessibilityLabel("Recording storage used")
            }
            Text(storage.folder)
                .font(.ui(.caption)).foregroundStyle(theme.textFaint)
                .lineLimit(1).truncationMode(.middle)
        }
        .padding(.horizontal, 18).padding(.vertical, 12)
        .frame(minWidth: 280, maxWidth: 360, alignment: .leading)
        .daylightRaised(theme, radius: 16)
    }
}

// MARK: Conflicts

private struct ConflictNotice: View {
    @Environment(\.auroraTheme) private var theme
    let conflicts: [RecordingConflict]
    let recordings: [Recording]

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "info.circle")
            VStack(alignment: .leading, spacing: 2) {
                Text(conflicts.count == 1 ? "A recording clashes" : "\(conflicts.count) clashes").bold()
                ForEach(conflicts.prefix(2), id: \.start) { c in
                    // The tail of the priority-ordered list is what loses, so name it.
                    Text("\(Format.dayLabel(c.start)) \(Format.clock(c.start))–\(Format.clock(c.stop)): \(c.slotIds.count) at once, \(c.overBy) more than your subscription allows. \(Text(titleOf(c.slotIds.last ?? 0)).italic()) will be dropped.")
                        .foregroundStyle(theme.textMuted)
                }
            }
        }
        .font(.ui(.callout).weight(.semibold))
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.orange.opacity(0.16), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.orange.opacity(0.45)))
        .accessibilityElement(children: .combine)
    }

    private func titleOf(_ id: Int64) -> String {
        recordings.first { $0.id == id }?.title ?? "#\(id)"
    }
}

// MARK: Rows

/// Run a change from a click, and say so when it fails (Windows' `guard`).
@MainActor
private func act(_ model: AppModel, _ whenItFails: String,
                 _ body: @escaping @Sendable (AuroraCore) throws -> Void) {
    Task {
        if let message = await model.changeRecordings(body).failureMessage {
            model.error = "\(whenItFails). \(message)"
        }
    }
}

private struct RowShell<Content: View>: View {
    @Environment(\.auroraTheme) private var theme
    var accent: Color?
    @ViewBuilder let content: Content

    var body: some View {
        HStack(spacing: 12) { content }
            .padding(.horizontal, 16).padding(.vertical, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .daylightRaised(theme, radius: 16)
            .overlay(alignment: .leading) {
                if let accent {
                    UnevenRoundedRectangle(topLeadingRadius: 16, bottomLeadingRadius: 16)
                        .fill(accent).frame(width: 4)
                }
            }
    }
}

/// The channel's logo, the title, and when it aired (Windows' `TitleBlock`).
private struct TitleBlock: View {
    @Environment(\.auroraTheme) private var theme
    let rec: Recording

    var body: some View {
        HStack(spacing: 12) {
            ChannelTile(name: rec.channelName ?? rec.title, number: nil, logo: rec.channelLogo, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text("\(Text(rec.title).fontWeight(.heavy))\(Text(episode.map { " · \($0)" } ?? "").foregroundStyle(theme.textFaint))")
                    .lineLimit(1)
                Text(whenAired(rec)).font(.ui(.callout)).foregroundStyle(theme.textMuted)
            }
            Spacer(minLength: 0)
        }
    }

    private var episode: String? {
        guard let s = rec.season, let e = rec.episode else { return nil }
        return String(format: "S%02dE%02d", s, e)
    }
}

/// "BBC One · Today 21:00–22:00".
private func whenAired(_ rec: Recording) -> String {
    (rec.channelName.map { "\($0) · " } ?? "")
        + "\(Format.dayLabel(rec.airStart)) \(Format.clock(rec.airStart))–\(Format.clock(rec.airStop))"
}

private struct RecordedCard: View {
    @Environment(\.auroraTheme) private var theme
    @Environment(AppModel.self) private var model
    let rec: Recording
    let play: () -> Void
    /// Deleting erases the file, with no undo, so it asks first, in the card.
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            art
            VStack(alignment: .leading, spacing: 2) {
                Text(rec.title).font(.ui(.headline).weight(.heavy)).lineLimit(1)
                Text(whenAired(rec)).font(.ui(.callout)).foregroundStyle(theme.textMuted).lineLimit(1)
            }
            if confirming {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Delete permanently, freeing \(Format.bytes(rec.bytes))?")
                        .font(.ui(.callout).bold()).foregroundStyle(theme.danger)
                    HStack {
                        Button("Delete", role: .destructive) {
                            let id = rec.id
                            act(model, "That did not work") { _ = try $0.dvrDelete(id: id) }
                        }
                        .buttonStyle(.daylight(.danger, .sm))
                        Button("Keep") { confirming = false }.buttonStyle(.daylight(.secondary, .sm))
                    }
                }
            } else {
                // Windows' one row where it fits; where the TV's larger buttons leave no
                // room, the length and size go above them rather than being cut to "403…".
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) {
                        meta.fixedSize()
                        Spacer(minLength: 4)
                        actions
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        meta
                        actions
                    }
                }
                .buttonStyle(.daylight(.secondary, .sm))
            }
        }
        .padding(14)
        .daylightRaised(theme, radius: 20)
        .overlay {
            if confirming { RoundedRectangle(cornerRadius: 20).strokeBorder(theme.danger, lineWidth: 2) }
        }
    }

    private var meta: some View {
        Text("\(Format.duration(rec.durationSecs)) · \(Format.bytes(rec.bytes))")
            .font(.ui(.caption).bold()).foregroundStyle(theme.textMuted).lineLimit(1)
    }

    private var actions: some View {
        HStack(spacing: 6) {
            Button {
                let (id, keep) = (rec.id, !rec.keep)
                act(model, "That did not work") { try $0.dvrSetKeep(id: id, keep: keep) }
            } label: {
                Image(systemName: rec.keep ? "heart.fill" : "heart")
            }
            .buttonStyle(.daylight(.secondary, .sm, iconOnly: true))
            .accessibilityLabel(rec.keep ? "Allow deleting to free space" : "Keep — never delete for space")
            Button { confirming = true } label: { Image(systemName: "xmark") }
                .buttonStyle(.daylight(.secondary, .sm, iconOnly: true))
                .accessibilityLabel("Delete \(rec.title)")
            Button("Play", systemImage: "play.fill", action: play)
                .buttonStyle(.daylight(.primary, .sm))
                .lineLimit(1).fixedSize()
        }
    }

    /// The channel's colour, its logo when it has one, and the tags.
    private var art: some View {
        ZStack(alignment: .topLeading) {
            ChannelTile.hue(rec.channelName ?? rec.title)
            AsyncImage(url: rec.channelLogo.flatMap(URL.init(string:))) { image in
                image.resizable().scaledToFit().opacity(0.9)
            } placeholder: { Color.clear }
                .containerRelativeFrame(.horizontal) { w, _ in w * 0.34 }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(.vertical, 24)
            HStack(spacing: 6) {
                // A completed-but-short recording still plays: say so before it ends early.
                if rec.reason != nil { chip("Cut short") }
                if !rec.watched {
                    DaylightBadge("New", tone: .new)
                }
                if rec.keep { chip("Kept") }
            }
            .padding(12)
        }
        .aspectRatio(2, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 16))
    }

    private func chip(_ text: String) -> some View {
        Text(text).font(.ui(.caption).bold()).padding(.horizontal, 8).padding(.vertical, 3)
            .background(.white, in: Capsule()).foregroundStyle(.black)
    }
}

/// Each scheduled recording with a REC chip, the channel, when, and whether a series
/// rule or a one-off asked for it.
private struct ComingUp: View {
    @Environment(\.auroraTheme) private var theme
    let upcoming: [Recording]
    let showRules: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Coming up").displayFont(.title2)
                Spacer()
                Button("Series rules", action: showRules).buttonStyle(.daylight(.secondary, .sm))
            }
            .padding(.bottom, 8)
            ForEach(upcoming, id: \.id) { r in
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 16) {
                        recChip
                        Text(r.subTitle.map { "\(r.title): \($0)" } ?? r.title).fontWeight(.heavy).lineLimit(1)
                        Spacer(minLength: 0)
                        Text(r.channelName ?? "").foregroundStyle(theme.textMuted).lineLimit(1)
                        Text(when(r)).fontWeight(.heavy).lineLimit(1)
                        kindChip(r)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        HStack { recChip; Text(r.subTitle.map { "\(r.title): \($0)" } ?? r.title).fontWeight(.heavy).lineLimit(1) }
                        HStack {
                            Text(r.channelName ?? "").foregroundStyle(theme.textMuted).lineLimit(1)
                            // The time whole on one line; the channel gives way, as
                            // Windows' grid ellipsises it first.
                            Text(when(r)).fontWeight(.heavy).lineLimit(1).fixedSize().layoutPriority(1)
                            Spacer(minLength: 0)
                            kindChip(r)
                        }
                    }
                }
                .padding(.vertical, 12)
                Divider()
            }
        }
        .padding(.horizontal, 24).padding(.top, 20).padding(.bottom, 12)
        .daylightRaised(theme, radius: 20)
        .padding(.top, 16)
    }

    private var recChip: some View {
        DaylightBadge("REC", tone: .live)
    }

    private func kindChip(_ r: Recording) -> some View {
        DaylightBadge(r.ruleId != nil ? "Series rule" : "Once")
    }

    private func when(_ r: Recording) -> String {
        "\(Format.dayLabel(r.airStart)) \(Format.clock(r.airStart))–\(Format.clock(r.airStop))"
    }
}

private struct ScheduledRow: View {
    @Environment(\.auroraTheme) private var theme
    @Environment(AppModel.self) private var model
    let rec: Recording

    private var live: Bool { rec.state == .recording }

    var body: some View {
        RowShell(accent: live ? theme.live : nil) {
            TitleBlock(rec: rec)
            Ticking(every: 1) { context in
                HStack(spacing: 8) {
                    if live {
                        Label("Recording", systemImage: "record.circle.fill")
                            .font(.ui(.caption).bold()).padding(.horizontal, 8).padding(.vertical, 3)
                            .background(theme.live, in: Capsule()).foregroundStyle(theme.liveText)
                        let elapsed = max(0, context.date.timeIntervalSince1970 - Double(rec.start))
                        ProgressView(value: min(1, elapsed / Double(max(1, rec.stop - rec.start))))
                            .frame(width: 120)
                            .accessibilityLabel("Recording \(rec.title)")
                        Text(Format.bytes(Int64(clamping: rec.liveBytes ?? UInt64(max(0, rec.bytes)))))
                            .font(.ui(.callout)).foregroundStyle(theme.textFaint)
                    } else {
                        Text(Format.until(rec.start, now: context.date))
                            .font(.ui(.callout)).foregroundStyle(theme.textMuted).lineLimit(1)
                    }
                }
            }
            Button(live ? "Stop" : "Cancel") {
                let id = rec.id
                act(model, "Could not stop that recording") { _ = try $0.dvrCancel(id: id) }
            }
            .buttonStyle(.daylight(.secondary, .sm))
        }
    }
}

private struct ProblemRow: View {
    @Environment(\.auroraTheme) private var theme
    @Environment(AppModel.self) private var model
    let rec: Recording

    var body: some View {
        RowShell(accent: theme.danger) {
            VStack(alignment: .leading, spacing: 2) {
                TitleBlock(rec: rec)
                if let reason = rec.reason {
                    Text(reason).font(.ui(.callout)).foregroundStyle(theme.danger)
                }
            }
            DaylightBadge(rec.state == .skipped ? "Skipped" : "Failed", tone: .outline)
            Button {
                let id = rec.id
                act(model, "Could not dismiss that recording") { _ = try $0.dvrDelete(id: id) }
            } label: {
                Image(systemName: "xmark")
            }
            .buttonStyle(.daylight(.ghost, .sm, iconOnly: true))
            .accessibilityLabel("Dismiss \(rec.title)")
        }
    }
}

/// One series rule. Deleting it asks first, as deleting a recording does: it sits beside
/// Pause, the reversible version of the same intent.
private struct RuleRow: View {
    @Environment(\.auroraTheme) private var theme
    @Environment(AppModel.self) private var model
    let rule: SeriesRule
    @State private var confirming = false

    var body: some View {
        RowShell(accent: confirming ? theme.danger : nil) {
            VStack(alignment: .leading, spacing: 2) {
                Text(rule.title).fontWeight(.bold)
                Text(summary).font(.ui(.callout)).foregroundStyle(theme.textMuted)
            }
            .opacity(rule.enabled ? 1 : 0.55)
            Spacer(minLength: 0)
            if confirming {
                Text("Stop recording \(rule.title) and forget this rule?")
                    .font(.ui(.callout)).foregroundStyle(theme.danger)
                Button("Delete rule", role: .destructive) {
                    let id = rule.id
                    act(model, "Could not delete that rule") { _ = try $0.dvrDeleteRule(id: id) }
                }
                .buttonStyle(.daylight(.danger, .sm))
                Button("Keep") { confirming = false }.buttonStyle(.daylight(.secondary, .sm))
            } else {
                // "tracked", not "recorded": the count is rows in any state (R-119).
                Text("\(rule.scheduled) tracked").font(.ui(.callout)).foregroundStyle(theme.textFaint)
                Button(rule.enabled ? "Pause" : "Resume") {
                    let (id, enabled) = (rule.id, !rule.enabled)
                    act(model, rule.enabled ? "Could not pause that rule" : "Could not resume it") {
                        try $0.dvrSetRuleEnabled(id: id, enabled: enabled)
                    }
                }
                .buttonStyle(.daylight(.secondary, .sm))
                Button { confirming = true } label: { Image(systemName: "xmark") }
                    .buttonStyle(.daylight(.ghost, .sm, iconOnly: true))
                    .accessibilityLabel("Delete the rule for \(rule.title)")
            }
        }
    }

    private var summary: String {
        var text = rule.channelId != nil ? (rule.channelName ?? "One channel") : "Any channel"
        if rule.newOnly { text += " · New episodes only" }
        if let days = rule.weekdays {
            text += " · " + days.map { Int($0) < weekdays.count ? weekdays[Int($0)] : "?" }.joined(separator: ", ")
        }
        if let minute = rule.aroundLocalMinute {
            text += String(format: " · around %02d:%02d", minute / 60, minute % 60)
        }
        if let keep = rule.keepEpisodes { text += " · keep \(keep)" }
        return text
    }
}

// MARK: Formatting

/// Windows' `lib/format.ts`, for the numbers and times these screens show.
enum Format {
    /// "1.4 GB": one decimal below 10, binary units, "0 MB" for nothing (`bytes`).
    static func bytes(_ n: Int64) -> String {
        guard n > 0 else { return "0 MB" }
        let units = ["B", "KB", "MB", "GB", "TB"]
        var i = 0
        var v = Double(n)
        func shown(_ value: Double, _ unit: Int) -> String {
            value < 10 && unit > 1 ? String(format: "%.1f", value) : String(Int(value.rounded()))
        }
        while (Double(shown(v, i)) ?? 0) >= 1024 && i < units.count - 1 {
            v /= 1024
            i += 1
        }
        return "\(shown(v, i)) \(units[i])"
    }

    /// "1:02:03", or "2:03" under an hour (`duration`).
    static func duration(_ secs: Int64) -> String {
        let s = max(0, secs)
        let (h, m, r) = (s / 3600, (s % 3600) / 60, s % 60)
        return h > 0 ? String(format: "%lld:%02lld:%02lld", h, m, r) : String(format: "%lld:%02lld", m, r)
    }

    /// Today, Tomorrow, Yesterday, or "Wed 19 Nov" (`dayLabel`).
    static func dayLabel(_ unix: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unix))
        let cal = Calendar.current
        let diff = cal.dateComponents([.day], from: cal.startOfDay(for: Date()), to: cal.startOfDay(for: date)).day ?? 0
        switch diff {
        case 0: return "Today"
        case 1: return "Tomorrow"
        case -1: return "Yesterday"
        default: return date.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
        }
    }

    /// The time of day, in the viewer's 12- or 24-hour style (`clockTime`).
    static func clock(_ unix: Int64) -> String {
        clockTime(unix: unix)
    }

    /// "in 4 min", "in 3h 20m", or a day and time (`untilLabel`).
    static func until(_ unix: Int64, now: Date = Date()) -> String {
        let secs = unix - Int64(now.timeIntervalSince1970)
        if secs <= 0 { return "now" }
        if secs < 3600 { return "in \(max(1, Int((Double(secs) / 60).rounded()))) min" }
        let (h, m) = (secs / 3600, (secs % 3600) / 60)
        if secs < 86400 { return m > 0 ? "in \(h)h \(m)m" : "in \(h)h" }
        return "\(dayLabel(unix)) at \(clock(unix))"
    }
}
