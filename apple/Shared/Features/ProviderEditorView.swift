import SwiftUI

/// Edit, check or remove a provider (Windows' `ProviderEditor`, README §15).
///
/// The password is shown only on request. It is the viewer's own subscription password on
/// their own device, and an account they cannot re-read is one they cannot correct after a
/// typo or a provider changing it -- but a settings screen that shows it by default is one
/// nobody can share their screen from.
struct ProviderEditorView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let provider: Provider

    @State private var loaded = false
    @State private var name = ""
    @State private var url = ""
    @State private var username = ""
    @State private var password = ""
    @State private var revealed = false
    @State private var checked: ProviderCheck?
    @State private var checking = false
    @State private var busy = false
    @State private var confirmingRemove = false
    @State private var problem: String?

    private var isXtream: Bool { provider.kind == "xtream" }

    var body: some View {
        Form {
            if loaded {
                Section {
                    LabeledContent("Name") {
                        TextField("Name", text: $name)
                    }
                    LabeledContent("Address") {
                        TextField("Address", text: $url)
                            .textContentType(.URL)
                            .autocorrectionDisabled()
                            #if os(iOS)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            #endif
                    }
                    if isXtream {
                        LabeledContent("Username") {
                            TextField("Username", text: $username)
                                .textContentType(.username)
                                .autocorrectionDisabled()
                                #if os(iOS)
                                .textInputAutocapitalization(.never)
                                #endif
                        }
                        LabeledContent("Password") {
                            HStack {
                                Group {
                                    if revealed {
                                        TextField("Password", text: $password)
                                            .autocorrectionDisabled()
                                            #if os(iOS)
                                            .textInputAutocapitalization(.never)
                                            #endif
                                    } else {
                                        SecureField("Password", text: $password)
                                    }
                                }
                                .textContentType(.password)
                                Button(revealed ? "Hide" : "Show") { revealed.toggle() }
                                    .accessibilityLabel(revealed ? "Hide the password" : "Show the password")
                                    #if os(iOS)
                                    .buttonStyle(.borderless)
                                    #else
                                    .buttonStyle(.bordered)
                                    #endif
                            }
                        }
                    }
                }
                Section {
                    Button(checking ? "Checking…" : "Check connection", systemImage: "network") {
                        Task { await check() }
                    }
                    .disabled(busy || checking || url.trimmingCharacters(in: .whitespaces).isEmpty)
                    if let checked {
                        CheckResult(result: checked) { suggested in
                            url = suggested
                            Task { await check() }
                        }
                    }
                } footer: {
                    Text("Tries what is in the form without saving it.")
                }
                Section {
                    Button("Save changes") { Task { await save() } }
                        .disabled(busy || name.trimmingCharacters(in: .whitespaces).isEmpty)
                } footer: {
                    Text("Refresh the provider afterwards to import from the new settings.")
                }
                Section {
                    Button("Remove provider", role: .destructive) { confirmingRemove = true }
                        .disabled(busy)
                }
            } else if problem == nil {
                ProgressView()
            }
            if let problem {
                Section {
                    Text(problem).foregroundStyle(.red)
                }
            }
        }
        .auroraPageTitle(provider.name)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        // Two steps: this also deletes everything the provider imported, with no undo.
        .confirmationDialog("Remove \(provider.name) and its \((provider.channelCount + provider.movieCount + provider.seriesCount).formatted()) imported items?",
                            isPresented: $confirmingRemove, titleVisibility: .visible) {
            Button("Yes, remove", role: .destructive) {
                Task {
                    busy = true
                    await model.removeProvider(provider.id)
                    dismiss()
                }
            }
            Button("Keep", role: .cancel) {}
        } message: {
            Text("Its channels, films, series and guide are removed from this device, and its password from the Keychain.")
        }
        .task { await load() }
    }

    private var draft: ProviderDraft {
        ProviderDraft(
            name: name,
            // Only Xtream and playlists can be added here, so a provider is one or the other.
            kind: isXtream ? "xtream" : "m3u",
            url: url,
            username: isXtream ? username : nil,
            password: isXtream ? password : nil
        )
    }

    private func load() async {
        do {
            let settings = try await model.providerSettings(provider.id)
            name = settings.name
            url = settings.url
            username = settings.username
            password = settings.password ?? ""
            loaded = true
        } catch {
            problem = error.viewerMessage
        }
    }

    private func check() async {
        checking = true
        defer { checking = false }
        checked = await model.checkProvider(draft)
    }

    private func save() async {
        busy = true
        defer { busy = false }
        problem = nil
        do {
            try await model.updateProvider(provider.id, draft: draft)
            dismiss()
        } catch {
            problem = error.viewerMessage
        }
    }
}

/// What "Check connection" found, in Windows' words, with its offer of an address that
/// does work when it found one (the editor's whole reason to exist: a provider saved with
/// an address that has stopped working).
struct CheckResult: View {
    let result: ProviderCheck
    let use: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(result.message, systemImage: result.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                .foregroundStyle(result.ok ? .green : .red)
                .font(.ui(.headline))
            if let detail = result.detail {
                Text(detail).font(.ui(.caption)).foregroundStyle(.secondary)
            }
            if result.ok, let summary = accountSummary {
                Text(summary).font(.ui(.caption)).foregroundStyle(.secondary)
            }
            if !result.ok, let suggested = result.suggestedUrl {
                Text(suggestion(suggested)).font(.ui(.caption))
                Button(result.suggestedKind == "m3u" ? "Use its playlist instead" : "Use that address") {
                    use(suggested)
                }
                #if os(iOS)
                .buttonStyle(.bordered)
                #endif
            }
        }
        .padding(.vertical, 4)
    }

    private func suggestion(_ url: String) -> String {
        if result.suggestedKind == "m3u" {
            return "Its playlist does work, even though its API does not. Most other players use the playlist and never touch the API, which is why this subscription works elsewhere."
        }
        var text = "It does answer at \(url)."
        if url.hasPrefix("http://") {
            text += " That is an unencrypted address, so your sign-in would be sent in clear text — which is how most panels work."
        }
        return text
    }

    /// Expiry and streams, when the panel said.
    private var accountSummary: String? {
        var parts: [String] = []
        if result.isTrial { parts.append("Trial") }
        if let days = result.daysUntilExpiry {
            parts.append(days <= 0 ? "Expired" : "Expires in \(days) day\(days == 1 ? "" : "s")")
        }
        if let max = result.maxConnections {
            parts.append("\(max) stream\(max == 1 ? "" : "s") at once")
        }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}
