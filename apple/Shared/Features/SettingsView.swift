import SwiftUI

/// Providers, and what the library holds.
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var adding = false

    var body: some View {
        NavigationStack {
            List {
                Section("Providers") {
                    ForEach(model.providers, id: \.id) { provider in
                        ProviderRow(provider: provider)
                    }
                    Button("Add a provider", systemImage: "plus") { adding = true }
                }
                if let progress = model.importProgress {
                    Section("Importing") {
                        Text(progress.phase.label)
                        if progress.total > 0 {
                            ProgressView(value: Double(progress.done), total: Double(progress.total))
                        }
                    }
                }
                Section("About") {
                    LabeledContent("Aurora core", value: coreVersion())
                    if let core = model.core {
                        LabeledContent("Library", value: core.dataDir())
                            .font(.caption)
                    }
                }
            }
            .navigationTitle("Settings")
            .sheet(isPresented: $adding) { NavigationStack { AddProviderView() } }
        }
    }
}

private struct ProviderRow: View {
    @Environment(AppModel.self) private var model
    let provider: Provider

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(provider.name).font(.headline)
            Text("\(provider.channelCount) channels · \(provider.movieCount) films · \(provider.seriesCount) series")
                .font(.caption).foregroundStyle(.secondary)
            if let at = provider.lastRefreshAt {
                Text("Refreshed \(Date(timeIntervalSince1970: TimeInterval(at)), style: .relative) ago")
                    .font(.caption2).foregroundStyle(.tertiary)
            }
            if let max = provider.maxConnections {
                Text("\(max) stream\(max == 1 ? "" : "s") at once").font(.caption2).foregroundStyle(.tertiary)
            }
            Button("Refresh", systemImage: "arrow.clockwise") {
                Task { await refresh() }
            }
            .disabled(model.importProgress != nil)
        }
    }

    private func refresh() async {
        // Refresh what this provider already holds: films and series only if it has any,
        // so a live-only Apple TV library stays live-only.
        let content = ImportContent(live: true, vod: provider.movieCount > 0, series: provider.seriesCount > 0)
        await model.refresh(providerId: provider.id, content: content)
    }
}
