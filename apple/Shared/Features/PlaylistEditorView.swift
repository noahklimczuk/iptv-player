import SwiftUI

/// The playlist editor (README §7.3): rename, renumber, regroup, hide, and put back.
/// The Apple counterpart of `src-ui` `PlaylistPage`.
///
/// One screen for all three lists, because the edits are the same whether a row is a
/// channel, a film or a series. It is the only screen that lists hidden rows: hiding
/// has to be reversible, and a viewer who cannot find what they hid has lost it. Search
/// and the bulk action work on the whole match, not the page on screen.
struct PlaylistEditorView: View {
    @Environment(AppModel.self) private var model
    @State private var kind: LibraryKind = .channels
    @State private var text = ""
    @State private var group: String?
    @State private var show: PlaylistShow = .all
    @State private var duplicatesOnly = false
    @State private var groups: [PlaylistGroup] = []
    @State private var rows: [PlaylistEntry] = []
    @State private var total: Int64 = 0
    @State private var loading = false
    @State private var editing: PlaylistEntry?
    @State private var copiesOf: PlaylistEntry?
    @State private var confirmingBulk = false

    /// Windows reads 300 at a time.
    private static let page: UInt32 = 300

    private var query: PlaylistQuery {
        PlaylistQuery(kind: kind, text: text.trimmingCharacters(in: .whitespaces).isEmpty ? nil : text,
                      group: group, show: show, duplicatesOnly: duplicatesOnly)
    }

    /// Everything the list depends on; a change starts it over.
    private struct Key: Hashable {
        let kind: LibraryKind, text: String, group: String?, show: PlaylistShow, dupes: Bool, version: Int
    }

    var body: some View {
        List {
            Section {
                Picker("List", selection: $kind) {
                    Text("Live TV").tag(LibraryKind.channels)
                    Text("Films").tag(LibraryKind.movies)
                    Text("Series").tag(LibraryKind.series)
                }
                .pickerStyle(.segmented)
                filters
                bulkActions
            } footer: {
                Text(summary)
            }
            Section {
                if rows.isEmpty && !loading {
                    ContentUnavailableView("Nothing here", systemImage: "line.3.horizontal.decrease",
                                           description: Text("Nothing matches what this list is narrowed to."))
                }
                ForEach(rows, id: \.id) { row in
                    entry(row)
                        .onAppear { if row.id == rows.last?.id { Task { await more() } } }
                }
                if loading { ProgressView().frame(maxWidth: .infinity) }
            }
        }
        .navigationTitle("Playlist")
        .searchable(text: $text, prompt: "Search this list")
        // A second press, as on Windows (R-125): it acts on every match, not the page.
        .confirmationDialog("Hide all \(total.formatted()) \(noun(total))?", isPresented: $confirmingBulk,
                            titleVisibility: .visible) {
            Button("Hide \(total.formatted())", role: .destructive) {
                Task { await model.playlistHideMatching(query, hidden: true) }
            }
        } message: {
            Text("Everything this list matches, not just what's on screen. They stay here, under Hidden, to put back.")
        }
        .sheet(item: $editing) { row in
            NavigationStack { EntryEditor(kind: kind, entry: row, groups: groups) }
                .parentalPinHost()
        }
        .sheet(item: $copiesOf) { row in
            NavigationStack { CopiesView(kind: kind, entry: row) }
        }
        .task(id: Key(kind: kind, text: text, group: group, show: show, dupes: duplicatesOnly,
                      version: model.libraryVersion)) {
            await reload()
        }
        .task(id: kind) {
            group = nil
            groups = await model.playlistGroups(kind)
        }
    }

    @ViewBuilder private var filters: some View {
        Picker("Group", selection: $group) {
            Text("All groups").tag(String?.none)
            ForEach(groups, id: \.name) { g in
                Text("\(g.name) (\(g.count.formatted()))").tag(Optional(g.name))
            }
        }
        Picker("Show", selection: $show) {
            Text("Everything").tag(PlaylistShow.all)
            Text("Shown").tag(PlaylistShow.visible)
            Text("Hidden").tag(PlaylistShow.hidden)
        }
        Toggle("Only what has other copies", isOn: $duplicatesOnly)
    }

    /// In the list rather than the toolbar: an iPhone hides the navigation bar while
    /// the search field is in use, and hiding everything a search matches is the point.
    private var bulkActions: some View {
        HStack {
            Button("Hide all \(total.formatted())", systemImage: "eye.slash") { confirmingBulk = true }
                .disabled(total == 0 || show == .hidden)
            Spacer()
            Button("Show all \(total.formatted())", systemImage: "eye") {
                Task { await model.playlistHideMatching(query, hidden: false) }
            }
            .disabled(total == 0 || show == .visible)
        }
        .buttonStyle(.bordered)
    }

    private var summary: String {
        "\(total.formatted()) \(noun(total))"
    }

    private func noun(_ n: Int64) -> String {
        switch kind {
        case .channels: n == 1 ? "channel" : "channels"
        case .movies: n == 1 ? "film" : "films"
        case .series: "series"
        }
    }

    private func entry(_ row: PlaylistEntry) -> some View {
        Button { editing = row } label: {
            HStack(spacing: 12) {
                if kind == .channels {
                    Text(row.number.map(String.init) ?? "–")
                        .font(.callout.monospacedDigit()).foregroundStyle(.secondary)
                        .frame(minWidth: 48, alignment: .trailing)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.name).foregroundStyle(row.hidden ? .secondary : .primary)
                    if row.name != row.providerName {
                        Text("Provider: \(row.providerName)").font(.caption).foregroundStyle(.secondary)
                    }
                    Text(details(row)).font(.caption).foregroundStyle(.tertiary)
                }
                Spacer()
                if row.edited {
                    Image(systemName: "pencil").foregroundStyle(.secondary)
                        .accessibilityLabel("Edited")
                }
                if row.hidden {
                    Image(systemName: "eye.slash").foregroundStyle(.secondary)
                        .accessibilityLabel("Hidden")
                }
            }
        }
        .buttonStyle(.plain)
        .contextMenu { actions(row) }
        #if os(iOS)
        .swipeActions(edge: .trailing) {
            Button(row.hidden ? "Show" : "Hide", systemImage: row.hidden ? "eye" : "eye.slash") {
                Task { await model.playlistSetHidden(kind, ids: [row.id], hidden: !row.hidden) }
            }
            .tint(row.hidden ? .green : .gray)
        }
        #endif
    }

    @ViewBuilder private func actions(_ row: PlaylistEntry) -> some View {
        Button(row.hidden ? "Show" : "Hide", systemImage: row.hidden ? "eye" : "eye.slash") {
            Task { await model.playlistSetHidden(kind, ids: [row.id], hidden: !row.hidden) }
        }
        Button("Edit", systemImage: "pencil") { editing = row }
        if row.duplicates > 0 {
            Button("Other copies (\(row.duplicates))", systemImage: "square.on.square") { copiesOf = row }
        }
        if row.edited {
            Button("Back to the provider's", systemImage: "arrow.uturn.backward") {
                Task { await model.playlistReset(kind, ids: [row.id]) }
            }
        }
    }

    /// "News · HD · English · 2 other copies"
    private func details(_ row: PlaylistEntry) -> String {
        var parts: [String] = []
        if let g = row.group, !g.isEmpty { parts.append(g) }
        if let q = row.quality { parts.append(q) }
        parts.append(row.lang.flatMap(languageName) ?? "Untagged")
        if row.duplicates > 0 {
            parts.append("\(row.duplicates) other cop\(row.duplicates == 1 ? "y" : "ies")")
        }
        if let p = row.provider { parts.append(p) }
        return parts.joined(separator: " · ")
    }

    private func reload() async {
        loading = true
        defer { loading = false }
        let page = await model.playlistPage(query, limit: Self.page, offset: 0)
        guard !Task.isCancelled else { return }
        rows = page?.rows ?? []
        total = page?.total ?? 0
    }

    private func more() async {
        guard !loading, Int64(rows.count) < total else { return }
        loading = true
        defer { loading = false }
        if let page = await model.playlistPage(query, limit: Self.page, offset: UInt32(rows.count)) {
            let known = Set(rows.map(\.id))
            rows += page.rows.filter { !known.contains($0.id) }
            total = page.total
        }
    }
}

/// "fr" → "French", from the system rather than a table of our own (as on Windows).
func languageName(_ code: String) -> String? {
    Locale.current.localizedString(forLanguageCode: code) ?? code.uppercased()
}

extension PlaylistEntry: Identifiable {}

/// One entry's name, number, group and logo. A cleared field hands it back to the
/// provider's value.
private struct EntryEditor: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let kind: LibraryKind
    let entry: PlaylistEntry
    let groups: [PlaylistGroup]
    @State private var name = ""
    @State private var number = ""
    @State private var group = ""
    @State private var logo = ""
    @State private var hidden = false
    @State private var saving = false

    /// The typed number, read as Windows reads it: `nil` is a typo, and nothing is sent.
    private var typedNumber: UInt32? { channelNumberFromTyped(input: number) }

    var body: some View {
        Form {
            Section {
                LabeledContent("Name") {
                    TextField("Name", text: $name, prompt: Text(entry.providerName))
                }
                if kind == .channels {
                    LabeledContent("Number") {
                        TextField("Number", text: $number, prompt: Text("Provider's numbering"))
                            #if os(iOS)
                            .keyboardType(.numberPad)
                            #endif
                    }
                    if typedNumber == nil {
                        Text("A channel number is digits only. Leave it empty for the provider's.")
                            .font(.caption).foregroundStyle(.red)
                    }
                }
                LabeledContent("Group") {
                    TextField("Group", text: $group, prompt: Text("Provider's group"))
                }
                if kind == .channels {
                    LabeledContent("Logo") {
                        TextField("Logo address", text: $logo, prompt: Text("Provider's logo"))
                            .textContentType(.URL)
                            #if os(iOS)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            #endif
                            .autocorrectionDisabled()
                    }
                }
                Toggle("Hidden", isOn: $hidden)
            } footer: {
                Text("Clear a field to go back to what the provider says. The provider calls this “\(entry.providerName)”.")
            }
            if entry.edited {
                Section {
                    Button("Put back everything the provider says", systemImage: "arrow.uturn.backward",
                           role: .destructive) {
                        Task {
                            saving = true
                            await model.playlistReset(kind, ids: [entry.id])
                            dismiss()
                        }
                    }
                }
            }
        }
        .navigationTitle(entry.name)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") { Task { await save() } }
                    .disabled(saving || (kind == .channels && typedNumber == nil))
            }
        }
        .disabled(saving)
        .onAppear {
            name = entry.name == entry.providerName ? "" : entry.name
            number = entry.number.map(String.init) ?? ""
            group = entry.group ?? ""
            logo = entry.logo ?? ""
            hidden = entry.hidden
        }
    }

    /// Only what changed goes in the patch: an untouched field is left alone, so an edit
    /// to the name doesn't pin today's provider number as an override.
    private func save() async {
        var patch = PlaylistPatch(name: nil, number: nil, group: nil, logo: nil, hidden: nil)
        let currentName = entry.name == entry.providerName ? "" : entry.name
        if name != currentName { patch.name = name }
        if kind == .channels, number != (entry.number.map(String.init) ?? "") {
            guard let typed = typedNumber else { return }
            patch.number = typed
        }
        if group != (entry.group ?? "") { patch.group = group }
        if kind == .channels, logo != (entry.logo ?? "") { patch.logo = logo }
        if hidden != entry.hidden { patch.hidden = hidden }
        saving = true
        let done = await model.playlistUpdate(kind, id: entry.id, patch: patch)
        saving = false
        if done { dismiss() }
    }
}

/// Every copy of one title, so the viewer can see what "one copy per title" chose
/// between, and hide the ones they don't want.
private struct CopiesView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let kind: LibraryKind
    let entry: PlaylistEntry
    @State private var copies: [Alternate] = []

    var body: some View {
        List(copies, id: \.id) { copy in
            VStack(alignment: .leading, spacing: 2) {
                Text(copy.name)
                Text([copy.quality, copy.provider].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary)
            }
            .contextMenu {
                Button("Hide this copy", systemImage: "eye.slash") {
                    Task {
                        await model.playlistSetHidden(kind, ids: [copy.id], hidden: true)
                        copies = await model.alternates(kind, id: entry.id)
                    }
                }
            }
        }
        .overlay {
            if copies.isEmpty { ContentUnavailableView("No other copies", systemImage: "square.on.square") }
        }
        .navigationTitle("Copies of \(entry.name)")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        .task { copies = await model.alternates(kind, id: entry.id) }
    }
}
