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
            LabeledContent("Version", value: Self.appVersion)
            LabeledContent("Aurora core", value: about.coreVersion)
            VStack(alignment: .leading, spacing: 2) {
                Text("Licence: \(about.license)")
                Text("Aurora is free software. You may study, change and share it.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text("Playback")
                Text("libmpv, built into the app as separate frameworks you can replace with your own build. It contains FFmpeg.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text("Metadata")
                Text("This product uses the TMDB API but is not endorsed or certified by TMDB.")
                    .font(.caption).foregroundStyle(.secondary)
            }
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
                        .font(.caption).foregroundStyle(.secondary)
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
        } header: {
            Text("About")
        }
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
                        .font(block.heading ? .headline : .footnote)
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
        .navigationTitle("Third-party notices")
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

    var body: some View {
        let diagnostics = model.core?.diagnostics()
        Section {
            if let replaced = diagnostics?.libraryWasReplaced {
                VStack(alignment: .leading, spacing: 4) {
                    Label("Your library was started again", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                    Text("The database could not be read at launch, so Aurora began a new one. Favourites, watch history and recording metadata went with it — refresh your provider to rebuild the library. The old file was kept at \(replaced).")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            Button {
                export()
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Label(exporting ? "Exporting…" : "Export logs", systemImage: "doc.text")
                    Text("Copies this run's log, with every address's login removed, for a bug report.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            .disabled(exporting)
            if let exported {
                #if os(tvOS)
                LabeledContent("Written to", value: exported.path)
                    .font(.caption)
                #else
                ShareLink(item: exported) {
                    Label("Share \(exported.lastPathComponent)", systemImage: "square.and.arrow.up")
                }
                #endif
            }
            if let failure {
                Text(failure).font(.caption).foregroundStyle(.red)
            }
        } header: {
            Text("Diagnostics")
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
