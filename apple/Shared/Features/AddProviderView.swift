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
