import SwiftUI

/// The Gemini key, and the one place that says why the rail is not on Home (Windows'
/// `GeminiPanel`, under its "Recommendations" heading, same wording). The rail itself is
/// silent by design, so this section carries every explanation.
struct RecommendationsSection: View {
    @Environment(AppModel.self) private var model
    @State private var status: GeminiStatus?
    @State private var key = ""
    @State private var busy = false
    /// What the last attempt to generate said, good or bad.
    @State private var result: (ok: Bool, text: String)?

    var body: some View {
        Section {
            SettingsHead("Recommendations").id("Recommendations")
            Text("Suggests films and shows from what you have watched, then keeps only the ones your provider actually carries. What leaves this device is the titles you have watched, their years and genres, how much of each you watched and whether you liked it — nothing about your provider, your sign-in or the rest of your library.")
                .font(.caption).foregroundStyle(.secondary)
            if let status {
                // The switch that actually stops anything being sent: clearing the key
                // cannot, because a build that ships one falls back to it.
                Toggle(isOn: Binding(get: { status.enabled }, set: { on in Task { await setEnabled(on) } })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Suggestions and the assistant").fontWeight(.heavy)
                        Text(status.enabled
                             ? "On. Your watch history is sent to Google when the home screen builds the rail."
                             : "Off. Nothing is sent to Google, including by the key built into this build.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                .disabled(busy)
                notes(status)
                keyRows(status)
                if status.hasKey {
                    Button(busy ? "Asking…" : "Get recommendations now", systemImage: "sparkles") {
                        Task { await test() }
                    }
                    .disabled(busy)
                }
            }
            if let result {
                Label(result.text, systemImage: result.ok ? "checkmark.circle" : "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(result.ok ? .green : .red)
            }
        }
        .task(id: model.profileId) {
            result = nil
            status = await model.geminiStatus()
        }
    }

    @ViewBuilder
    private func notes(_ status: GeminiStatus) -> some View {
        if status.enabled && !status.hasKey {
            Text("No key yet. One is free from aistudio.google.com/apikey.")
                .font(.caption).foregroundStyle(.secondary)
        }
        if status.hasKey && status.keyIsBuiltIn {
            Text("Using the key built into this build. Entering your own replaces it, and carries your own rate limit rather than sharing one. Removing a key you entered falls back to this one — the switch above is what stops anything being sent.")
                .font(.caption).foregroundStyle(.secondary)
        }
        if status.hasKey && !status.hasHistory {
            Text("Nothing watched yet. These are built from your history, so the rail appears once there is one.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func keyRows(_ status: GeminiStatus) -> some View {
        SecureField(status.hasKey ? "Replace the saved key…" : "Paste your API key…", text: $key)
            .textContentType(.password)
            .autocorrectionDisabled()
            .onSubmit { Task { await save() } }
        if !key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            // Not "Save": the metadata section on this screen has one, as on Windows.
            Button("Use this key") { Task { await save() } }
                .disabled(busy)
        }
        if status.hasKey && !status.keyIsBuiltIn {
            Button("Remove key", role: .destructive) {
                key = ""
                Task { await model.setGeminiKey(""); await reload() }
            }
            .disabled(busy)
        }
    }

    private func reload() async {
        status = await model.geminiStatus()
    }

    private func save() async {
        busy = true
        await model.setGeminiKey(key)
        key = ""
        result = nil
        await reload()
        busy = false
    }

    private func setEnabled(_ on: Bool) async {
        busy = true
        result = nil
        await model.setGeminiEnabled(on)
        await reload()
        busy = false
    }

    /// Ask for a fresh set, and say plainly what came back.
    private func test() async {
        busy = true
        result = nil
        switch await model.askForAiPicks() {
        case .success(let picks):
            let found = picks.items.count, missing = Int(picks.notInLibrary)
            result = found > 0
                ? (true, "Suggested \(found + missing) titles; \(found) are in your library and are on the home screen now.")
                : (false, "Suggested \(missing) titles and your provider carries none of them. That is a subscription that does not stock what you watch, not a broken key.")
        case .failure(let error):
            result = (false, error.viewerMessage)
        }
        busy = false
    }
}
