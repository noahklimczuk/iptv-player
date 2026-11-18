import SwiftUI

/// Add an Xtream panel or a playlist URL, then import it.
///
/// On Apple TV, typing here works with the Siri Remote and, better, with the Continuity
/// Keyboard on any nearby iPhone (docs §2.4). Pairing from a phone is Phase 5.
struct AddProviderView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    enum Kind: String, CaseIterable, Identifiable {
        case xtream = "Xtream Codes"
        case m3u = "Playlist URL"
        var id: Self { self }
    }

    @State private var kind: Kind = .xtream
    @State private var name = ""
    @State private var url = ""
    @State private var username = ""
    @State private var password = ""
    @State private var checked: ProviderCheck?
    @State private var checking = false
    #if os(tvOS)
    // Live TV only by default on Apple TV: a whole film catalogue and its search index
    // on storage the system can purge is docs §5 risk 3.
    @State private var importMovies = false
    @State private var importSeries = false
    #else
    @State private var importMovies = true
    @State private var importSeries = true
    #endif

    var body: some View {
        Form {
            Picker("Type", selection: $kind) {
                ForEach(Kind.allCases) { Text($0.rawValue).tag($0) }
            }
            Section {
                TextField("Name", text: $name)
                TextField(kind == .xtream ? "Server address" : "Playlist address", text: $url)
                    .textContentType(.URL)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    #endif
                    .task(id: url) {
                        // Windows asks a beat after typing stops (`DETECT_AFTER_TYPING_MS`):
                        // asked on every keystroke, a link typed by hand split itself at
                        // the first letter of its password.
                        try? await Task.sleep(for: .milliseconds(typingPause))
                        if !Task.isCancelled { recognise(url) }
                    }
                if kind == .xtream {
                    TextField("Username", text: $username)
                        .textContentType(.username)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        #endif
                    SecureField("Password", text: $password)
                        .textContentType(.password)
                }
            }
            Section {
                Button(checking ? "Checking…" : "Check connection", systemImage: "network") {
                    Task { await check() }
                }
                .disabled(checking || url.trimmingCharacters(in: .whitespaces).isEmpty)
                if let checked {
                    CheckResult(result: checked) { suggested in
                        if checked.suggestedKind == "m3u" { kind = .m3u }
                        url = suggested
                        Task { await check() }
                    }
                }
            } footer: {
                Text("A playlist link with a username and password in it fills in the form by itself.")
            }
            Section("Import") {
                Toggle("Live TV", isOn: .constant(true)).disabled(true)
                Toggle("Films", isOn: $importMovies)
                Toggle("Series", isOn: $importSeries)
            }
            Section {
                Button("Add and import") { Task { await add() } }
                    .disabled(!canAdd || model.importProgress != nil)
            }
        }
        .navigationTitle("Add a provider")
    }

    private var canAdd: Bool {
        let hasURL = !url.trimmingCharacters(in: .whitespaces).isEmpty
        return kind == .m3u ? hasURL : hasURL && !username.isEmpty && !password.isEmpty
    }

    /// A pasted `get.php` or `player_api.php` link carries the whole sign-in: split it
    /// into the form, as Windows' wizard does (`providers.detect`).
    /// Windows' `DETECT_AFTER_TYPING_MS`.
    private let typingPause = Int(detectAfterTypingMs())

    private func recognise(_ text: String) {
        let found = detectSource(text: text)
        guard found.kind == "xtream", let user = found.username, let pass = found.password else { return }
        kind = .xtream
        url = found.url
        username = user
        password = pass
        if name.trimmingCharacters(in: .whitespaces).isEmpty {
            name = URL(string: found.url)?.host() ?? ""
        }
    }

    private func check() async {
        checking = true
        defer { checking = false }
        checked = await model.checkProvider(ProviderDraft(
            name: name,
            kind: kind == .xtream ? "xtream" : "m3u",
            url: url,
            username: kind == .xtream ? username : nil,
            password: kind == .xtream ? password : nil
        ))
    }

    private func add() async {
        let content = ImportContent(live: true, vod: importMovies, series: importSeries)
        switch kind {
        case .xtream:
            await model.addXtream(name: name, url: url, username: username, password: password, content: content)
        case .m3u:
            await model.addM3u(name: name, url: url, content: content)
        }
        if model.error == nil { dismiss() }
    }
}
