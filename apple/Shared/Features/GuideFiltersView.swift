import SwiftUI

/// The guide's Filters: Windows' `GuideFiltersPanel` and the helpers in `guideFilters.ts`.
///
/// The chips on the guide are the quick way — tap a group in or out. This sheet is the
/// thorough way: every group in one searchable list, shown only or hidden, and the
/// filters that are about the channel rather than its group. Which channels each lets
/// through is the core's (`guide_filters.rs`), so the count here is the guide's own.
extension GuideFilters {
    static let none = GuideFilters(groups: [], groupMode: .only, withListings: false,
                                   favoritesOnly: false, catchupOnly: false, hideRadio: false,
                                   qualities: [])

    /// How many separate filters are on, for the badge on the Filters button.
    var activeCount: Int {
        [!groups.isEmpty, withListings, favoritesOnly, catchupOnly, hideRadio, !qualities.isEmpty]
            .filter { $0 }.count
    }

    /// Add a group to the choice, or take it out.
    func toggling(_ group: String) -> GuideFilters {
        var f = self
        if let i = f.groups.firstIndex(of: group) { f.groups.remove(at: i) } else { f.groups.append(group) }
        return f
    }

    func togglingQuality(_ quality: String) -> GuideFilters {
        var f = self
        if let i = f.qualities.firstIndex(of: quality) { f.qualities.remove(at: i) } else { f.qualities.append(quality) }
        return f
    }
}

/// Kept on this device, as Windows keeps it in `localStorage`: a filter is how somebody
/// likes to browse, not part of the library, and it should be there next time.
enum GuideFilterStore {
    private static let key = "aurora.guide.filters"

    static func read() -> GuideFilters {
        guideFiltersRead(text: UserDefaults.standard.string(forKey: key) ?? "")
    }

    static func write(_ filters: GuideFilters) {
        UserDefaults.standard.set(guideFiltersWrite(filters: filters), forKey: key)
    }
}

/// The Filters button, with how many filters are on.
struct GuideFiltersButton: View {
    let count: Int
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: "line.3.horizontal.decrease")
                Text("Filters")
                if count > 0 {
                    Text("\(count)")
                        .font(.caption.weight(.heavy))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 6)
                        .background(Color.accentColor, in: Capsule())
                }
            }
            .fontWeight(count > 0 ? .heavy : .regular)
        }
        .buttonStyle(.bordered)
        .tint(count > 0 ? .accentColor : .secondary)
        .accessibilityLabel(count > 0 ? "Filters, \(count) on" : "Filters")
    }
}

struct GuideFiltersSheet: View {
    let groups: [ChannelGroup]
    @Binding var filters: GuideFilters
    /// How many channels the filters leave.
    let shown: UInt32
    @State private var query = ""
    @Environment(\.dismiss) private var dismiss

    private var matching: [ChannelGroup] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? groups : groups.filter { $0.name.lowercased().contains(q) }
    }

    var body: some View {
        NavigationStack {
            Form {
                groupsSection
                Section("Channels") {
                    Toggle("With listings only", isOn: $filters.withListings)
                    Toggle("Favourites only", isOn: $filters.favoritesOnly)
                    Toggle("Catch-up only", isOn: $filters.catchupOnly)
                    Toggle("Hide radio", isOn: $filters.hideRadio)
                }
                Section("Quality") {
                    HStack {
                        ForEach(guideQualities(), id: \.self) { q in
                            FilterChip(title: q, selected: filters.qualities.contains(q)) {
                                filters = filters.togglingQuality(q)
                            }
                        }
                    }
                }
                Section {
                    Text("\(shown.formatted()) channel\(shown == 1 ? "" : "s") shown")
                        .foregroundStyle(.secondary)
                    Button("Reset all", role: .destructive) { filters = .none }
                        .disabled(filters.activeCount == 0)
                }
            }
            .searchable(text: $query, prompt: "Search \(groups.count) groups…")
            .navigationTitle("Guide filters")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }

    @ViewBuilder private var groupsSection: some View {
        Section {
            Picker("Chosen groups are", selection: $filters.groupMode) {
                Text("Show only").tag(GuideGroupMode.only)
                Text("Hide").tag(GuideGroupMode.hide)
            }
            .pickerStyle(.segmented)
            HStack {
                Button(query.isEmpty ? "Choose all" : "Choose all \(matching.count) found") {
                    var seen = Set(filters.groups)
                    filters.groups += matching.map(\.name).filter { seen.insert($0).inserted }
                }
                Spacer()
                Button(filters.groups.isEmpty ? "Clear" : "Clear (\(filters.groups.count))") {
                    filters.groups = []
                }
                .disabled(filters.groups.isEmpty)
            }
            // Each its own focus on tvOS, not one block for the row (see ProviderRow).
            #if os(iOS)
            .buttonStyle(.borderless)
            #else
            .buttonStyle(.bordered)
            #endif
            if matching.isEmpty {
                Text("No group matches “\(query)”.").foregroundStyle(.secondary)
            }
            ForEach(matching, id: \.name) { group in
                let chosen = filters.groups.contains(group.name)
                Button { filters = filters.toggling(group.name) } label: {
                    HStack {
                        Image(systemName: chosen ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(chosen ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                        // A hidden group says so: struck through rather than lit.
                        Text(group.name)
                            .strikethrough(chosen && filters.groupMode == .hide)
                            .foregroundStyle(chosen && filters.groupMode == .hide ? .secondary : .primary)
                            .lineLimit(1)
                        Spacer()
                        Text("\(group.count)").font(.caption).foregroundStyle(.secondary)
                    }
                }
                .accessibilityAddTraits(chosen ? .isSelected : [])
            }
        } header: {
            Text("Groups")
        }
    }
}
