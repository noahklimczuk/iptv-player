import SwiftUI

/// Settings → Privacy, as Windows' `PrivacyPanel` (README §15: "history retention, clear
/// history").
///
/// Watch history is what Continue Watching, the ticks on episodes and every
/// recommendation are read from. The core enforces both rows: the window is applied there
/// (at once, and every half hour after), and both sit behind the Settings lock, so a kids
/// profile cannot clear what it watched.
struct PrivacySection: View {
    @Environment(AppModel.self) private var model
    @State private var history: WatchHistory?
    @State private var confirming = false
    @State private var busy = false
    @State private var message: String?
    @State private var failure: String?

    /// The windows offered; nil keeps history for good, which is the default.
    private static let windows: [(days: UInt32?, label: String)] = [
        (nil, "Forever"),
        (365, "1 year"),
        (90, "90 days"),
        (30, "30 days"),
    ]

    var body: some View {
        Section {
            SettingsHead("Privacy").id("Privacy")
            if let history {
                SettingsField(label: "Keep watch history",
                              hint: "Anything not watched for longer than this is forgotten, on every device you sync with.") {
                    Segments(options: Self.windows.map { ($0.days, $0.label) },
                             selection: Binding(get: { history.days }, set: { choose($0) }))
                        .disabled(busy)
                }
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Clear watch history").fontWeight(.heavy)
                        Spacer()
                        Text(Self.entries(history.items))
                            .font(.caption.bold())
                            .padding(.horizontal, 8).padding(.vertical, 3)
                            .overlay(Capsule().strokeBorder(.secondary))
                    }
                    Text("\(who) Continue Watching, episode ticks and recommendations start again from nothing. Other profiles keep theirs.")
                        .font(.caption).foregroundStyle(.secondary)
                    // Two steps, because there is no undo, and the deletion syncs.
                    if confirming {
                        HStack {
                            Button("Yes, clear it", role: .destructive) { clear() }
                                .buttonStyle(.borderedProminent)
                            Button("Keep") { confirming = false }
                                .buttonStyle(.bordered)
                        }
                        .disabled(busy)
                    } else {
                        Button("Clear history") { confirming = true }
                            .buttonStyle(.bordered)
                            .disabled(busy || history.items == 0)
                    }
                }
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Usage data").fontWeight(.heavy)
                        Spacer()
                        Text("None sent")
                            .font(.caption.bold())
                            .padding(.horizontal, 8).padding(.vertical, 3)
                            .overlay(Capsule().strokeBorder(.secondary))
                    }
                    Text("Aurora sends none: there is no analytics or crash reporting to switch on. docs/PRIVACY.md lists everything that does leave this device.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if let failure {
                    Text(failure).font(.callout).foregroundStyle(.red)
                } else if let message {
                    Text(message).font(.callout).foregroundStyle(.green)
                }
            } else {
                ProgressView()
            }
        }
        .task(id: model.profileId) { await load() }
    }

    private var who: String {
        model.activeProfile.map { "\($0.name)'s" } ?? "This profile's"
    }

    static func entries(_ n: UInt64) -> String {
        "\(n.formatted()) \(n == 1 ? "title" : "titles")"
    }

    private func load() async {
        let core = model.core
        let profileId = model.profileId
        history = await Task.detached { try? core?.privacyHistory(profileId: profileId) }.value
    }

    private func choose(_ days: UInt32?) {
        let label = Self.windows.first { $0.days == days }?.label ?? ""
        run { core in
            let removed = try core.privacySetHistoryDays(days: days)
            if days == nil { return "Watch history is kept for good." }
            return removed > 0
                ? "Kept for \(label). Forgot \(Self.entries(removed)) older than that."
                : "Kept for \(label)."
        }
    }

    private func clear() {
        let profileId = model.profileId
        run { core in
            let removed = try core.privacyClearHistory(profileId: profileId)
            return removed > 0 ? "Forgot \(Self.entries(removed))." : "There was nothing to forget."
        }
    }

    /// Run a change through `AppModel.guarded`, which asks for the parental PIN when the
    /// core says Settings are locked, then read the history back.
    private func run(_ body: @escaping @Sendable (AuroraCore) throws -> String) {
        busy = true
        failure = nil
        Task {
            do {
                message = try await model.guarded(body)
            } catch {
                message = nil
                failure = error.viewerMessage
            }
            busy = false
            confirming = false
            await load()
            // Continue Watching and the rails are read from what was just forgotten.
            await model.reload()
        }
    }
}
