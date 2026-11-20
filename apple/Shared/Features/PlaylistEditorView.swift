import SwiftUI

/// The playlist editor (README §7.3): rename, renumber, regroup, hide, and put back.
/// The Apple counterpart of `src-ui` `PlaylistPage`, drawn as it is: the list's
/// segments, the filter bar, the bulk actions, and the table as one raised panel whose
/// cells edit in place, the way a spreadsheet's do.
///
/// One screen for all three lists, because the edits are the same whether a row is a
/// channel, a film or a series. It is the only screen that lists hidden rows: hiding
/// has to be reversible, and a viewer who cannot find what they hid has lost it. Search
/// and the bulk action work on the whole match, not the page on screen.
struct PlaylistEditorView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    @State private var kind: LibraryKind = .channels
    @State private var text = ""
    @State private var group: String?
    @State private var show: PlaylistShow = .all
    @State private var duplicatesOnly = false
    @State private var limit = Self.page
    @State private var groups: [PlaylistGroup] = []
    @State private var rows: [PlaylistEntry] = []
    @State private var total: Int64 = 0
    @State private var loading = false
    @State private var loadedOnce = false
    @State private var selected: Set<Int64> = []
    @State private var busy = false
    /// Whether "Hide all N matching" is waiting for its second press (R-125).
    @State private var confirmingBulk = false
    @State private var copiesOf: PlaylistEntry?
    @State private var typo: String?
    @State private var width: CGFloat = 0
    @FocusState private var searching: Bool

    /// Windows reads 300 at a time, and Show more adds 300.
    private static let page: UInt32 = 300

    /// Windows' table, with its columns, where there is the room for it; on an iPhone
    /// each row folds its group and language under the name.
    private var wide: Bool {
        #if os(tvOS)
        true
        #else
        width >= 760
        #endif
    }

    private var query: PlaylistQuery {
        PlaylistQuery(kind: kind, text: text.trimmingCharacters(in: .whitespaces).isEmpty ? nil : text,
                      group: group, show: show, duplicatesOnly: duplicatesOnly)
    }

    /// Everything the list depends on; a change starts it over.
    private struct Key: Hashable {
        let kind: LibraryKind, text: String, group: String?, show: PlaylistShow, dupes: Bool
        let limit: UInt32, version: Int
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Daylight.px(12)) {
                heading
                filterBar
                bulkBar
                panel
            }
            .padding(.horizontal, pagePadding)
            .padding(.bottom, Daylight.px(16))
            .foregroundStyle(theme.text)
            #if os(iOS)
            // Clicking away commits a cell, on Windows: here a tap on the page or a
            // scroll ends the editing, and the cell commits as its focus leaves.
            .background {
                Color.clear.contentShape(Rectangle()).onTapGesture {
                    UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder),
                                                    to: nil, from: nil, for: nil)
                }
            }
            #endif
        }
        #if os(iOS)
        .scrollDismissesKeyboard(.immediately)
        #endif
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .auroraTitle("Playlist")
        .sheet(item: $copiesOf) { row in
            NavigationStack { CopiesView(kind: kind, entry: row) }
                .auroraAppearance()
        }
        .alert("“\(typo ?? "")” is not a channel number", isPresented: Binding(
            get: { typo != nil }, set: { if !$0 { typo = nil } })) {
            Button("OK") { typo = nil }
        } message: {
            Text("Enter a whole number above zero, or clear the field to use the provider's own number.")
        }
        .task(id: Key(kind: kind, text: text, group: group, show: show, dupes: duplicatesOnly,
                      limit: limit, version: model.libraryVersion)) {
            await reload()
        }
        .task(id: kind) {
            groups = await model.playlistGroups(kind)
        }
        .onChange(of: text) { limit = Self.page }
    }

    private var pagePadding: CGFloat {
        #if os(tvOS)
        0
        #else
        width >= 760 ? 36 : 16
        #endif
    }

    // MARK: The bars

    /// The title and which list, side by side as on Windows (the iPhone's title is the
    /// navigation bar's).
    private var heading: some View {
        HStack(spacing: 20) {
            #if os(tvOS)
            Text("Playlist")
                .font(.largeTitle.weight(.heavy))
                .accessibilityAddTraits(.isHeader)
            #endif
            Segments(options: [(LibraryKind.channels, "Live TV"), (.movies, "Movies"), (.series, "Series")],
                     selection: Binding(get: { kind }, set: { switchKind($0) }))
        }
    }

    private func switchKind(_ next: LibraryKind) {
        guard next != kind else { return }
        kind = next
        selected = []
        group = nil
        limit = Self.page
        confirmingBulk = false
    }

    /// Search, Group, Show, Duplicates only, and the count.
    @ViewBuilder private var filterBar: some View {
        let controls = FlowLayout(spacing: 8) {
            searchField
            select("Group", value: group ?? "All groups", chosen: group != nil,
                   options: [("All groups", nil as String?, nil as String?)]
                    + groups.map { ($0.name, Optional($0.name), String($0.count)) }) { group = $0; limit = Self.page }
            select("Show", value: showLabel(show), chosen: true,
                   options: [PlaylistShow.all, .visible, .hidden].map { (showLabel($0), $0, nil as String?) }) {
                show = $0; limit = Self.page
            }
            Button("Duplicates only") { duplicatesOnly.toggle(); limit = Self.page }
                .buttonStyle(.daylight(duplicatesOnly ? .primary : .secondary))
                .accessibilityAddTraits(duplicatesOnly ? .isSelected : [])
        }
        if wide {
            HStack(alignment: .center, spacing: 12) {
                controls.frame(maxWidth: .infinity, alignment: .leading)
                count
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                controls
                count
            }
        }
    }

    private var count: some View {
        Text("\(String(total)) \(total == 1 ? "entry" : "entries")"
             + (Int64(rows.count) < total ? " · showing \(rows.count)" : ""))
            .font(.system(size: Daylight.px(13), weight: .semibold))
            .foregroundStyle(theme.textMuted)
            .fixedSize()
    }

    private func showLabel(_ show: PlaylistShow) -> String {
        switch show {
        case .all: "Shown and hidden"
        case .visible: "Shown only"
        case .hidden: "Hidden only"
        }
    }

    /// Windows' `aurora-field`: the raised surface, the accent's edge while typing. A TV
    /// draws its own pill behind a text field, so there it is the same surface as a
    /// button that brings up the keyboard.
    @ViewBuilder private var searchField: some View {
        #if os(tvOS)
        TVSearchField(text: $text)
        #else
        phoneSearchField
        #endif
    }

    private var phoneSearchField: some View {
        let shape = RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous)
        return TextField("Search this list", text: $text, prompt: Text("Search this list").foregroundStyle(theme.textFaint))
            .textFieldStyle(.plain)
            .font(.system(size: Daylight.px(15), weight: .medium))
            .autocorrectionDisabled()
            #if os(iOS)
            .textInputAutocapitalization(.never)
            .submitLabel(.search)
            #endif
            .focused($searching)
            .padding(.vertical, Daylight.px(10)).padding(.horizontal, Daylight.px(14))
            .frame(width: Daylight.px(wide ? 280 : 240))
            .background(theme.surface, in: shape)
            .overlay(shape.strokeBorder(searching ? theme.accent : .clear, lineWidth: 1.5))
            .accessibilityLabel("Search this list")
    }

    /// Windows' `Select`: a small button, the accent once something is chosen, opening
    /// the choices with their counts.
    private func select<V: Hashable>(_ label: String, value: String, chosen: Bool,
                                     options: [(String, V, String?)],
                                     set: @escaping (V) -> Void) -> some View {
        Menu {
            ForEach(Array(options.enumerated()), id: \.offset) { _, option in
                Button { set(option.1) } label: {
                    Text(option.0)
                    if let hint = option.2 { Text(hint) }
                }
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "chevron.down").font(.system(size: Daylight.px(13), weight: .bold))
                Text(value).frame(maxWidth: Daylight.px(200), alignment: .leading)
            }
        }
        .menuStyle(.button)
        .buttonStyle(.daylight(chosen ? .primary : .secondary, .sm))
        .accessibilityLabel("\(label): \(value)")
    }

    /// Bulk actions work on the selection, or on everything the search matched.
    private var bulkBar: some View {
        FlowLayout(spacing: 8) {
            Button {
                selected = allListedSelected ? [] : Set(rows.map(\.id))
            } label: {
                HStack(spacing: 8) {
                    DaylightCheckbox(isOn: allListedSelected)
                    Text("Select all listed").font(.system(size: Daylight.px(13), weight: .semibold))
                }
            }
            .buttonStyle(FocusRingStyle())
            .accessibilityLabel("Select everything listed")
            .accessibilityAddTraits(allListedSelected ? .isSelected : [])
            if !selected.isEmpty {
                let ids = Array(selected)
                DaylightBadge("\(ids.count) selected", tone: .accent)
                Button("Hide", systemImage: "xmark") { act { await model.playlistSetHidden(kind, ids: ids, hidden: true) } }
                    .buttonStyle(.daylight(.secondary, .sm))
                Button("Show", systemImage: "checkmark") { act { await model.playlistSetHidden(kind, ids: ids, hidden: false) } }
                    .buttonStyle(.daylight(.secondary, .sm))
                Button("Reset") { act { await model.playlistReset(kind, ids: ids) } }
                    .buttonStyle(.daylight(.ghost, .sm))
                Button("Clear selection") { selected = [] }
                    .buttonStyle(.daylight(.ghost, .sm))
            } else if total > Int64(rows.count) {
                // Asks first: its condition is the state the screen is in on arrival,
                // and undoing it is a page of select-and-reset per 300 (R-125).
                if confirmingBulk {
                    Text("Hide all \(String(total))? Undoing this is \((total + 299) / 300) pages of select-and-reset.")
                        .font(.system(size: Daylight.px(13)))
                        .foregroundStyle(theme.danger)
                        .fixedSize(horizontal: false, vertical: true)
                    Button("Hide them") {
                        confirmingBulk = false
                        let q = query
                        act { await model.playlistHideMatching(q, hidden: true) }
                    }
                    .buttonStyle(.daylight(.danger, .sm))
                    Button("Cancel") { confirmingBulk = false }
                        .buttonStyle(.daylight(.ghost, .sm))
                } else {
                    Button("Hide all \(String(total)) matching") { confirmingBulk = true }
                        .buttonStyle(.daylight(.secondary, .sm))
                }
            }
        }
        .disabled(busy)
        .frame(minHeight: Daylight.px(32))
    }

    private var allListedSelected: Bool {
        !rows.isEmpty && rows.allSatisfy { selected.contains($0.id) }
    }

    // MARK: The table

    /// Daylight: the table is one raised panel.
    private var panel: some View {
        VStack(spacing: 0) {
            if wide { columnHeads }
            Group {
                if loading && !loadedOnce {
                    VStack(spacing: 6) {
                        ForEach(0..<12, id: \.self) { _ in
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .fill(theme.surfaceSunk)
                                .frame(height: Daylight.px(38))
                        }
                    }
                    .padding(.top, 8)
                } else if rows.isEmpty {
                    VStack(spacing: 8) {
                        Text("Nothing here").font(.system(size: Daylight.px(17), weight: .heavy))
                        Text(text.isEmpty && group == nil && !duplicatesOnly
                             ? "This list is empty until a provider has been imported."
                             : "No entry in this list matches those filters.")
                            .font(.system(size: Daylight.px(15)))
                            .foregroundStyle(theme.textMuted)
                            .multilineTextAlignment(.center)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, Daylight.px(48))
                } else {
                    LazyVStack(spacing: 0) {
                        ForEach(rows, id: \.id) { row in
                            PlaylistRow(row: row, kind: kind, wide: wide,
                                        selected: selected.contains(row.id),
                                        toggle: { toggle(row.id) },
                                        patch: { patch(row, $0) },
                                        typo: { typo = $0 },
                                        hide: { hidden in act { await model.playlistSetHidden(kind, ids: [row.id], hidden: hidden) } },
                                        reset: { act { await model.playlistReset(kind, ids: [row.id]) } },
                                        copies: { copiesOf = row })
                        }
                    }
                }
            }
            .padding(.horizontal, Daylight.px(16))
            if Int64(rows.count) < total {
                Button("Show more") { limit += Self.page }
                    .buttonStyle(.daylight(.secondary, .sm))
                    .padding(Daylight.px(16))
            }
        }
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity)
        .daylightRaised(theme, radius: Daylight.px(24))
    }

    private var columnHeads: some View {
        HStack(spacing: Daylight.px(12)) {
            Color.clear.frame(width: Daylight.px(20), height: 1)
            if kind == .channels { Text("No.").frame(width: Daylight.px(64), alignment: .leading) }
            Text("Name").frame(maxWidth: .infinity, alignment: .leading)
            Text("Group").frame(width: Daylight.px(160), alignment: .leading)
            if kind == .channels { Text("Logo").frame(width: Daylight.px(200), alignment: .leading) }
            Text("Quality").frame(width: Daylight.px(120), alignment: .leading)
            Text("Language").frame(width: Daylight.px(110), alignment: .leading)
            Text("Shown").frame(width: Daylight.px(150), alignment: .trailing)
        }
        .font(.system(size: Daylight.px(13), weight: .bold))
        .foregroundStyle(theme.textMuted)
        .padding(.vertical, Daylight.px(10)).padding(.horizontal, Daylight.px(16))
        .accessibilityHidden(true)
    }

    private func toggle(_ id: Int64) {
        if selected.contains(id) { selected.remove(id) } else { selected.insert(id) }
    }

    private func patch(_ row: PlaylistEntry, _ patch: PlaylistPatch) {
        act { await model.playlistUpdate(kind, id: row.id, patch: patch) }
    }

    /// Run one edit; the library's version moving on re-reads the list, because the core
    /// decides what the row looks like afterwards.
    private func act(_ edit: @escaping () async -> some Any) {
        busy = true
        Task {
            _ = await edit()
            busy = false
        }
    }

    private func reload() async {
        loading = true
        defer { loading = false }
        let page = await model.playlistPage(query, limit: limit, offset: 0)
        guard !Task.isCancelled else { return }
        rows = page?.rows ?? []
        total = page?.total ?? 0
        loadedOnce = true
        // A selection outlives a re-read only where its rows are still listed.
        selected = selected.intersection(rows.map(\.id))
    }
}

/// One row of Windows' table: its cells edit in place, the row dims when hidden, and
/// the switch on the right shows or hides it.
private struct PlaylistRow: View {
    let row: PlaylistEntry
    let kind: LibraryKind
    let wide: Bool
    let selected: Bool
    let toggle: () -> Void
    let patch: (PlaylistPatch) -> Void
    let typo: (String) -> Void
    let hide: (Bool) -> Void
    let reset: () -> Void
    let copies: () -> Void
    @Environment(\.auroraTheme) private var theme

    private static let empty = PlaylistPatch(name: nil, number: nil, group: nil, logo: nil, hidden: nil)

    var body: some View {
        Group {
            if wide { wideRow } else { narrowRow }
        }
        .opacity(row.hidden ? 0.5 : 1)
        .overlay(alignment: .bottom) { Rectangle().fill(theme.border).frame(height: 1) }
        .contextMenu {
            Button(row.hidden ? "Show" : "Hide", systemImage: row.hidden ? "eye" : "eye.slash") { hide(!row.hidden) }
            if row.duplicates > 0 {
                Button("Other copies (\(row.duplicates))", systemImage: "square.on.square", action: copies)
            }
            if row.edited {
                Button("Back to the provider's", systemImage: "arrow.uturn.backward", action: reset)
            }
        }
    }

    private var wideRow: some View {
        HStack(spacing: Daylight.px(12)) {
            checkbox
            if kind == .channels { numberCell.frame(width: Daylight.px(64)) }
            nameCell.frame(maxWidth: .infinity, alignment: .leading)
            groupCell.frame(width: Daylight.px(160), alignment: .leading)
            if kind == .channels { logoCell.frame(width: Daylight.px(200), alignment: .leading) }
            badges.frame(width: Daylight.px(120), alignment: .leading)
            language.frame(width: Daylight.px(110), alignment: .leading)
            HStack(spacing: 6) {
                if row.edited { resetButton }
                shownSwitch
            }
            .frame(width: Daylight.px(150), alignment: .trailing)
        }
        .frame(minHeight: Daylight.px(46))
    }

    /// An iPhone's row: the number and name, the group and language under the name,
    /// then the badges and the switch.
    private var narrowRow: some View {
        HStack(spacing: 10) {
            checkbox
            if kind == .channels { numberCell.frame(width: 52) }
            VStack(alignment: .leading, spacing: 0) {
                nameCell
                HStack(spacing: 4) {
                    Text([row.group, row.lang.flatMap(languageName) ?? "Untagged"]
                        .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
                        .lineLimit(1)
                }
                .font(.system(size: 12))
                .foregroundStyle(theme.textMuted)
                .padding(.horizontal, 6)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            badges
            if row.edited { resetButton }
            shownSwitch
        }
        .padding(.vertical, 6)
        .frame(minHeight: 46)
    }

    private var checkbox: some View {
        Button(action: toggle) { DaylightCheckbox(isOn: selected) }
            .buttonStyle(FocusRingStyle(radius: Daylight.px(5)))
            .accessibilityLabel("Select \(row.name)")
            .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var numberCell: some View {
        EditableCell(value: row.number.map { String($0) } ?? "", label: "Number for \(row.name)",
                     weight: .regular, numeric: true) { next in
            guard let number = channelNumberFromTyped(input: next) else {
                typo(next.trimmingCharacters(in: .whitespaces))
                return
            }
            var p = Self.empty
            p.number = number
            patch(p)
        }
    }

    private var nameCell: some View {
        VStack(alignment: .leading, spacing: 0) {
            EditableCell(value: row.name, label: "Name for \(row.name)") { next in
                var p = Self.empty
                p.name = next
                patch(p)
            }
            if wide, row.edited, row.providerName != row.name {
                Text(row.providerName)
                    .font(.system(size: Daylight.px(12)))
                    .foregroundStyle(theme.textFaint)
                    .lineLimit(1)
                    .padding(.horizontal, 6)
                    .accessibilityLabel("The provider calls it \(row.providerName)")
            }
        }
    }

    @ViewBuilder private var groupCell: some View {
        if kind == .channels {
            EditableCell(value: row.group ?? "", label: "Group for \(row.name)", weight: .regular) { next in
                var p = Self.empty
                p.group = next
                patch(p)
            }
        } else {
            Text(row.group ?? "—")
                .font(.system(size: Daylight.px(13)))
                .foregroundStyle(theme.textMuted)
                .lineLimit(1)
        }
    }

    /// The logo, which the provider often gets wrong: a broken address collapses its
    /// slot rather than showing a torn picture.
    private var logoCell: some View {
        HStack(spacing: 6) {
            Group {
                if let logo = row.logo, let url = URL(string: logo) {
                    AsyncImage(url: url) { phase in
                        if let image = phase.image { image.resizable().scaledToFit() } else { Color.clear }
                    }
                } else {
                    Color.clear
                }
            }
            .frame(width: Daylight.px(28), height: Daylight.px(28))
            .clipShape(RoundedRectangle(cornerRadius: 4))
            .accessibilityHidden(true)
            EditableCell(value: row.logo ?? "", label: "Logo for \(row.name)", weight: .regular, url: true) { next in
                var p = Self.empty
                p.logo = next
                patch(p)
            }
        }
    }

    private var badges: some View {
        HStack(spacing: 6) {
            if let quality = row.quality { DaylightBadge(quality) }
            if row.duplicates > 0 {
                Button(action: copies) { DaylightBadge("+\(row.duplicates)", tone: .outline) }
                    .buttonStyle(FocusRingStyle(radius: Daylight.px(12)))
                    .accessibilityLabel("\(row.duplicates) other cop\(row.duplicates == 1 ? "y" : "ies") of this entry")
            }
        }
    }

    @ViewBuilder private var language: some View {
        if let name = row.lang.flatMap(languageName) {
            Text(name).font(.system(size: Daylight.px(13))).foregroundStyle(theme.textMuted).lineLimit(1)
        } else {
            Text("Untagged").font(.system(size: Daylight.px(13))).foregroundStyle(theme.textFaint)
        }
    }

    private var resetButton: some View {
        Button("Reset", action: reset)
            .buttonStyle(.daylight(.ghost, .sm))
            .accessibilityHint("Back to the provider's values")
    }

    /// Blue when shown: almost every row is, and the hidden few dim with their row.
    private var shownSwitch: some View {
        DaylightSwitch(isOn: !row.hidden, label: "Show \(row.name)") { hide(!row.hidden) }
    }
}

/// A cell that edits in place: Return commits, and so does moving away; an unchanged
/// value sends nothing. The same bargain every spreadsheet makes.
private struct EditableCell: View {
    let value: String
    let label: String
    var weight: Font.Weight = .semibold
    var numeric = false
    var url = false
    let commit: (String) -> Void
    @Environment(\.auroraTheme) private var theme
    @State private var draft = ""
    /// What was last handed on, so Return and the focus leaving don't both send it.
    @State private var sent: String?
    @FocusState private var focused: Bool

    #if os(tvOS)
    @State private var editing = false
    #endif

    var body: some View {
        #if os(tvOS)
        // A TV draws its own pill behind every text field, so the cell is flat text,
        // as on Windows, and Select brings up the keyboard.
        Button {
            draft = value
            editing = true
        } label: {
            Group {
                if value.isEmpty {
                    Text("—").foregroundStyle(theme.textFaint)
                } else {
                    Text(value)
                }
            }
            .font(.system(size: Daylight.px(15), weight: weight))
            .lineLimit(1)
            .padding(.horizontal, 6).padding(.vertical, 3)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(8)))
        .accessibilityLabel(label)
        .alert(label, isPresented: $editing) {
            TextField(label, text: $draft)
                .keyboardType(numeric ? .numberPad : url ? .URL : .default)
            Button("Save") { finish() }
            Button("Cancel", role: .cancel) { draft = value }
        } message: {
            Text(numeric ? "Clear it to use the provider's own number." : "Clear it to use the provider's own.")
        }
        .onChange(of: value) { sent = nil }
        #else
        let shape = RoundedRectangle(cornerRadius: Daylight.px(8), style: .continuous)
        TextField(label, text: $draft, prompt: Text("—").foregroundStyle(theme.textFaint))
            .textFieldStyle(.plain)
            .font(.system(size: Daylight.px(15), weight: focused ? .regular : weight))
            .lineLimit(1)
            .autocorrectionDisabled()
            .keyboardType(numeric ? .numberPad : url ? .URL : .default)
            .textInputAutocapitalization(url ? .never : .sentences)
            .focused($focused)
            .padding(.horizontal, 6).padding(.vertical, 3)
            .background(focused ? theme.surface : .clear, in: shape)
            .overlay(shape.strokeBorder(focused ? theme.accent : .clear, lineWidth: 1))
            .accessibilityLabel(label)
            .onAppear { draft = value }
            .onChange(of: value) { _, now in
                sent = nil
                if !focused { draft = now }
            }
            .onSubmit(finish)
            .onChange(of: focused) { _, now in if !now { finish() } }
        #endif
    }

    private func finish() {
        guard draft != value, draft != sent else { return }
        sent = draft
        commit(draft)
    }
}

#if os(tvOS)
private struct TVSearchField: View {
    @Binding var text: String
    @Environment(\.auroraTheme) private var theme
    @State private var draft = ""
    @State private var editing = false

    var body: some View {
        Button {
            draft = text
            editing = true
        } label: {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                Text(text.isEmpty ? "Search this list" : text).lineLimit(1)
                Spacer(minLength: 0)
            }
            .font(.system(size: Daylight.px(15), weight: .medium))
            .foregroundStyle(text.isEmpty ? theme.textFaint : theme.text)
            .padding(.vertical, Daylight.px(10)).padding(.horizontal, Daylight.px(14))
            .frame(width: Daylight.px(280), alignment: .leading)
            .background(theme.surface, in: RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous))
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(14)))
        .accessibilityLabel("Search this list")
        .accessibilityValue(text)
        .alert("Search this list", isPresented: $editing) {
            TextField("Search this list", text: $draft)
            Button("Search") { text = draft }
            if !text.isEmpty { Button("Clear", role: .destructive) { text = "" } }
            Button("Cancel", role: .cancel) {}
        }
    }
}
#endif

/// "fr" → "French", from the system rather than a table of our own (as on Windows).
func languageName(_ code: String) -> String? {
    Locale.current.localizedString(forLanguageCode: code) ?? code.uppercased()
}

extension PlaylistEntry: Identifiable {}

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
        .auroraPageTitle("Copies of \(entry.name)")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        .task { copies = await model.alternates(kind, id: entry.id) }
    }
}
