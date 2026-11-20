import OSLog
import SwiftUI

/// What this build is, and the licences it is obliged to carry (Windows' `AboutPanel`,
/// same wording). The notices are read from the files bundled with the app, so what is
/// on screen is what was shipped.
struct AboutSection: View {
    private let about = aboutCore()
    private let notices = Notices.load()

    var body: some View {
        Section {
            SettingsHead("About").id("About")
            // Windows' definition list: the term faint in a column of its own.
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 6) {
                term("Version") { Text(Self.appVersion).monospacedDigit() }
                term("Aurora core") { Text(about.coreVersion).monospacedDigit() }
                term("Licence") {
                    let free = Text(" — Aurora is free software. You may study, change and share it.")
                        .foregroundStyle(.tertiary)
                    Text("\(about.license)\(free)")
                }
                term("Playback") {
                    Text("libmpv, built into the app as separate frameworks you can replace with your own build. It contains FFmpeg.")
                        .font(.ui(.subheadline)).foregroundStyle(.secondary)
                }
                term("Metadata") {
                    Text("This product uses the TMDB API but is not endorsed or certified by TMDB.")
                        .font(.ui(.subheadline)).foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 4)
            if let notices {
                NavigationLink {
                    NoticesView(text: notices)
                } label: {
                    Label("Third-party notices", systemImage: "info.circle")
                }
            } else {
                // A packaging fault, as on Windows: a build that cannot show the notices it
                // is obliged to carry should not be distributed.
                VStack(alignment: .leading, spacing: 2) {
                    Label("The licence notices are missing", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                    Text("This build should carry THIRD-PARTY-NOTICES.md and does not. That is a packaging fault — please report it.")
                        .font(.ui(.caption)).foregroundStyle(.secondary)
                }
            }
            #if os(tvOS)
            LabeledContent("Source and releases", value: about.releasesUrl)
            #else
            if let url = URL(string: about.releasesUrl) {
                Link(destination: url) {
                    Label("Source and releases", systemImage: "shippingbox")
                }
            }
            #endif
        }
    }

    private func term(_ name: String, @ViewBuilder _ value: () -> some View) -> some View {
        GridRow {
            Text(name)
                .font(.ui(.subheadline))
                .foregroundStyle(.tertiary)
                .frame(width: Self.termWidth, alignment: .leading)
            value()
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// Windows' 160-pixel column, a little narrower on a phone.
    private static var termWidth: CGFloat {
        #if os(tvOS)
        240
        #else
        UIDevice.current.userInterfaceIdiom == .phone ? 96 : 160
        #endif
    }

    private static var appVersion: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String
        return build.map { "\(short) (\($0))" } ?? short
    }
}

private enum Notices {
    /// The Apple build's notices, then the shared file they refer to.
    static func load() -> String? {
        let parts = ["THIRD-PARTY-NOTICES-APPLE", "THIRD-PARTY-NOTICES"].compactMap { name in
            Bundle.main.url(forResource: name, withExtension: "md")
                .flatMap { try? String(contentsOf: $0, encoding: .utf8) }
        }
        return parts.count == 2 ? parts.joined(separator: "\n\n---\n\n") : nil
    }
}

private struct NoticesView: View {
    let text: String

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                    Text(block.text)
                        .font(.ui(block.heading ? .headline : .footnote))
                        .padding(.top, block.heading ? 8 : 0)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        #if os(tvOS)
                        // The remote scrolls by moving focus, so each paragraph takes it.
                        .focusable()
                        #else
                        .textSelection(.enabled)
                        #endif
                }
            }
            .padding()
        }
        .auroraPageTitle("Third-party notices")
    }

    private struct Block {
        var text: AttributedString
        var heading: Bool
    }

    /// The Markdown files as readable paragraphs: hard-wrapped lines joined, list items
    /// kept on their own lines, headings set apart, and bold, code and links rendered.
    private var blocks: [Block] {
        text.components(separatedBy: "\n\n").compactMap { raw in
            let lines = raw.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
            guard let first = lines.first, !first.isEmpty, first != "---" else { return nil }
            if first.hasPrefix("#") {
                let title = lines.joined(separator: " ").drop(while: { $0 == "#" || $0 == " " })
                return Block(text: Self.inline(String(title)), heading: true)
            }
            var joined = ""
            for line in lines {
                let item = line.hasPrefix("- ") || line.hasPrefix("* ") || line.first?.isNumber == true && line.contains(". ")
                if joined.isEmpty {
                    joined = item && line.hasPrefix("- ") ? "• " + line.dropFirst(2) : line
                } else if item {
                    joined += "\n" + (line.hasPrefix("- ") ? "• " + line.dropFirst(2) : line)
                } else {
                    joined += " " + line
                }
            }
            return Block(text: Self.inline(joined), heading: false)
        }
    }

    private static func inline(_ markdown: String) -> AttributedString {
        (try? AttributedString(markdown: markdown,
                               options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(markdown)
    }
}

/// The things a support message needs, and the one thing a viewer has to be told
/// (Windows' `DiagnosticsPanel`, same wording where the platform allows).
struct DiagnosticsSection: View {
    @Environment(AppModel.self) private var model
    @State private var engine: String?
    @State private var exporting = false
    @State private var exported: URL?
    @State private var failure: String?

    #if !os(tvOS)
    private var exportButton: some View {
        Button(exporting ? "Exporting…" : "Export logs", systemImage: "doc.text") { export() }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(exporting)
    }

    private var exportNote: some View {
        Text("Copies this run's log, with every address's login removed, for a bug report.")
            .font(.ui(.subheadline)).foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
    }
    #endif

    var body: some View {
        let diagnostics = model.core?.diagnostics()
        Section {
            SettingsHead("Diagnostics").id("Diagnostics")
            if let replaced = diagnostics?.libraryWasReplaced {
                VStack(alignment: .leading, spacing: 4) {
                    Label("Your library was started again", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                    Text("The database could not be read at launch, so Aurora began a new one. Favourites, watch history and recording metadata went with it — refresh your provider to rebuild the library. The old file was kept at \(replaced).")
                        .font(.ui(.caption)).foregroundStyle(.secondary)
                }
            }
            #if os(tvOS)
            Button {
                export()
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Label(exporting ? "Exporting…" : "Export logs", systemImage: "doc.text")
                    Text("Copies this run's log, with every address's login removed, for a bug report.")
                        .font(.ui(.caption)).foregroundStyle(.secondary)
                }
            }
            .disabled(exporting)
            #else
            // Windows' small button with its note beside it, faint.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) { exportButton; exportNote }
                VStack(alignment: .leading, spacing: 8) { exportButton; exportNote }
            }
            #endif
            if let exported {
                #if os(tvOS)
                LabeledContent("Written to", value: exported.path)
                    .font(.ui(.caption))
                #else
                ShareLink(item: exported) {
                    Label("Share \(exported.lastPathComponent)", systemImage: "square.and.arrow.up")
                }
                #endif
            }
            if let failure {
                Text(failure).font(.ui(.caption)).foregroundStyle(.red)
            }
        } footer: {
            VStack(alignment: .leading, spacing: 2) {
                if let diagnostics { Text("Data folder: \(diagnostics.dataDir)") }
                Text("Video engine: \(engine ?? "…")")
            }
        }
        .task {
            engine = await Task.detached { MPVPlayerController.engineVersion() ?? "mpv (version unknown)" }.value
        }
    }

    private func export() {
        exporting = true
        failure = nil
        let folder = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("logs-export", isDirectory: true)
        Task {
            let result = await Task.detached { () -> Result<URL, Error> in
                Result { try LogExport.write(to: folder) }
            }.value
            exporting = false
            switch result {
            case .success(let url): exported = url
            case .failure(let error): failure = "Could not export the logs. \(error.localizedDescription)"
            }
        }
    }
}

/// This run's lines from the system log, Aurora's own and mpv's, made safe to hand over.
enum LogExport {
    /// When this run began, so the export reads only its own stretch of the system log:
    /// from the last boot it took over a minute in the simulator.
    nonisolated(unsafe) static var launchedAt = Date()

    static func write(to folder: URL) throws -> URL {
        let store = try OSLogStore(scope: .currentProcessIdentifier)
        let entries = try store.getEntries(
            at: store.position(date: launchedAt.addingTimeInterval(-5)),
            matching: NSPredicate(format: "subsystem == %@", "tv.aurora"))
        let stamp = ISO8601DateFormatter()
        var text = ""
        for case let entry as OSLogEntryLog in entries {
            text += "\(stamp.string(from: entry.date)) \(label(entry.level)) [\(entry.category)] \(entry.composedMessage)\n"
        }
        if text.isEmpty { text = "Nothing was logged in this run.\n" }
        // The shared crates and the player redact as they log; this is the second pass
        // Windows' export does not need, because here nothing else wrote the log.
        let safe = redactLog(text: text)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let name = "aurora-\(Int(Date().timeIntervalSince1970)).log"
        let url = folder.appendingPathComponent(name)
        try safe.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    private static func label(_ level: OSLogEntryLog.Level) -> String {
        switch level {
        case .fault: "FAULT"
        case .error: "ERROR"
        case .notice: "NOTICE"
        case .info: "INFO"
        case .debug: "DEBUG"
        default: "-"
        }
    }
}
