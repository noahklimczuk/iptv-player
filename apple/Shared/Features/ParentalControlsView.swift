import SwiftUI

/// Settings → Parental controls, as Windows' `ParentalPanel` (README §11): the parental
/// PIN, what is hidden from everybody, each profile's age and daily limits, and the
/// channels and categories that are locked.
///
/// Every change goes straight to the core, which asks for the parental PIN when one is
/// set; the dialog for that comes from `AppModel.guarded`, so nothing here checks first.
struct ParentalControlsView: View {
    @Environment(AppModel.self) private var model
    @State private var busy = false
    @State private var failure: String?

    var body: some View {
        List {
            if let settings = model.parentalSettings {
                MasterPinSection(settings: settings, busy: $busy, run: run)
                Section {
                    toggle("Hide adult categories",
                           hint: settings.hasMasterPin || !settings.hideAdult
                               ? "For every profile. Categories named adult, XXX or 18+ are left out of every list and refused if played. A profile with an age limit always hides them."
                               : "For every profile. Set a parental PIN first to show them: only the PIN can show adult categories.",
                           isOn: settings.hideAdult) { on in
                        run { try $0.setParental(hideAdult: on, lockSettings: settings.lockSettings, masterPin: .keep) }
                    }
                    // Showing them needs the PIN (README §11, F-39); hiding them never does.
                    .disabled(settings.hideAdult && !settings.hasMasterPin)
                    toggle("Ask for the PIN to open Settings",
                           hint: settings.hasMasterPin
                               ? "Settings stays closed until the parental PIN is entered."
                               : "Set a parental PIN first.",
                           isOn: settings.lockSettings) { on in
                        run { try $0.setParental(hideAdult: settings.hideAdult, lockSettings: on, masterPin: .keep) }
                    }
                    .disabled(!settings.hasMasterPin)
                    toggle("Hide locked channels and categories",
                           hint: "Off shows them with a padlock and asks for the PIN when they are played. On leaves them out of every list until the PIN is entered.",
                           isOn: model.parental?.hideLocked ?? false) { on in
                        run { _ = try $0.parentalSetHideLocked(hide: on) }
                    }
                    .disabled(!settings.hasMasterPin)
                }
                ageLimits(pinSet: settings.hasMasterPin)
                LocksSection(pinSet: settings.hasMasterPin, busy: $busy, run: run)
            } else {
                ProgressView()
            }
            if let failure {
                Text(failure).foregroundStyle(.red)
            }
        }
        .auroraPageTitle("Parental controls")
        .disabled(busy)
        .task { await model.loadParental() }
    }

    /// Run a change, then read everything back: the core is the one that knows.
    private func run(_ body: @escaping @Sendable (AuroraCore) throws -> Void) {
        busy = true
        failure = nil
        Task {
            failure = await model.changeParental(body)
            busy = false
        }
    }

    @ViewBuilder private func ageLimits(pinSet: Bool) -> some View {
        Section {
            ForEach(model.profiles, id: \.id) { profile in
                ProfileLimits(profile: profile, pinSet: pinSet, run: run)
            }
        } header: {
            Text("Age limits")
        } footer: {
            if let state = model.parental, state.titles > 0, state.ratedTitles < state.titles {
                Text((state.ratedTitles == 0
                      ? "None of your films and series carry a rating yet"
                      : "\(state.ratedTitles.formatted()) of \(state.titles.formatted()) films and series carry a rating")
                     + ". Providers rarely send one, so an age limit can only judge what Artwork and metadata has fetched, and a profile that blocks unrated titles sees only those.")
            }
        }
    }

    private func toggle(_ title: String, hint: String, isOn: Bool, set: @escaping (Bool) -> Void) -> some View {
        Toggle(isOn: Binding(get: { isOn }, set: set)) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                Text(hint).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// The limits a profile can have, as a minimum age (`aurora_core::parental`). Windows'
/// `AGE_LIMITS` in `ParentalPanel.tsx`, checked against it by `mirrors.rs`.
private let ageLimits: [Limit<UInt8>] = [
    .init(nil, "No limit"),
    .init(0, "U — suitable for all"),
    .init(6, "Up to PG"),
    .init(12, "Up to 12"),
    .init(15, "Up to 15"),
    .init(18, "Up to 18"),
]

/// A day's viewing, in minutes (F-37). Windows' `DAILY_LIMITS`.
private let dailyLimits: [Limit<UInt32>] = [
    .init(nil, "No daily limit"),
    .init(30, "30 minutes a day"),
    .init(60, "1 hour a day"),
    .init(90, "1½ hours a day"),
    .init(120, "2 hours a day"),
    .init(180, "3 hours a day"),
    .init(240, "4 hours a day"),
]

/// One choice of limit: its value (`nil` is none) and what the picker calls it.
private struct Limit<Value: Hashable>: Hashable {
    let value: Value?
    let label: String
    init(_ value: Value?, _ label: String) {
        self.value = value
        self.label = label
    }
}

private struct MasterPinSection: View {
    let settings: ParentalSettings
    @Binding var busy: Bool
    let run: (@escaping @Sendable (AuroraCore) throws -> Void) -> Void

    @State private var open = false
    @State private var pin = ""
    @State private var again = ""

    private static func isDigit(_ c: Character) -> Bool { c.isASCII && c.isNumber }
    private var ready: Bool { pin.count == 4 && pin == again }

    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 4) {
                Text("Parental PIN")
                Text(settings.hasMasterPin
                     ? "Set. It opens locked channels, and guards these settings and adding or removing profiles."
                     : "Not set, so anybody can change these settings and add an unrestricted profile. Set one before relying on an age limit.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if open {
                field("New parental PIN", text: $pin)
                field("Again", text: $again)
                if again.count == 4 && pin != again {
                    Text("The two PINs differ.").foregroundStyle(.red)
                }
                Button("Save PIN") { save(.set(pin: pin)) }.disabled(!ready)
                Button("Cancel", role: .cancel) { reset() }
            } else {
                Button(settings.hasMasterPin ? "Change PIN" : "Set a PIN", systemImage: "lock") { open = true }
                if settings.hasMasterPin {
                    Button("Remove PIN", systemImage: "lock.open", role: .destructive) { save(.remove) }
                }
            }
        }
    }

    private func field(_ title: String, text: Binding<String>) -> some View {
        SecureField(title, text: text, prompt: Text(title == "Again" ? "Again" : "4 digits"))
            #if os(iOS)
            .keyboardType(.numberPad)
            #endif
            .onChange(of: text.wrappedValue) { text.wrappedValue = String(text.wrappedValue.filter(Self.isDigit).prefix(4)) }
    }

    private func save(_ change: MasterPinChange) {
        let settings = settings
        run {
            try $0.setParental(
                // Clearing the PIN hides adult categories again: only the PIN can show them.
                hideAdult: change == .remove ? true : settings.hideAdult,
                // Without a PIN nothing could open a locked Settings again.
                lockSettings: change == .remove ? false : settings.lockSettings,
                masterPin: change)
        }
        reset()
    }

    private func reset() {
        pin = ""
        again = ""
        open = false
    }
}

private struct ProfileLimits: View {
    let profile: Profile
    let pinSet: Bool
    let run: (@escaping @Sendable (AuroraCore) throws -> Void) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(profile.name).font(.headline)
            Picker("Age limit", selection: Binding(get: { profile.maxAge }, set: { age in
                // No limit lets everything through, unrated titles included.
                set(maxAge: age, allowUnrated: age == nil ? true : profile.allowUnrated)
            })) {
                ForEach(ageLimits, id: \.label) { Text($0.label).tag($0.value) }
            }
            if profile.maxAge != nil {
                Toggle(isOn: Binding(get: { profile.allowUnrated },
                                     set: { set(maxAge: profile.maxAge, allowUnrated: $0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Allow titles with no rating")
                        Text("Provider listings often leave it out, so these are usually missing data rather than suitable.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            // A daily limit holds only while a parental PIN exists: the PIN is what gives
            // more time, and without one the profile on the clock could lift it itself.
            Picker("Daily limit", selection: Binding(get: { profile.dailyLimitMin }, set: { daily in
                set(maxAge: profile.maxAge, allowUnrated: profile.allowUnrated, daily: daily)
            })) {
                ForEach(dailyLimits, id: \.label) { Text($0.label).tag($0.value) }
            }
            .disabled(!pinSet)
            if !pinSet {
                Text("Set a parental PIN first: it is what gives more time.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func set(maxAge: UInt8?, allowUnrated: Bool, daily: UInt32?? = nil) {
        let id = profile.id
        let dailyLimit = daily ?? profile.dailyLimitMin
        run {
            try $0.setProfileLimits(profileId: id, maxAge: maxAge, allowUnrated: allowUnrated,
                                    dailyLimitMin: dailyLimit)
        }
    }
}

/// What is locked, and locking a category or a channel found by name.
private struct LocksSection: View {
    @Environment(AppModel.self) private var model
    let pinSet: Bool
    @Binding var busy: Bool
    let run: (@escaping @Sendable (AuroraCore) throws -> Void) -> Void

    @State private var groups: [PlaylistGroup] = []
    @State private var group = ""
    @State private var text = ""
    @State private var found: [PlaylistEntry] = []

    var body: some View {
        Section("Locked") {
            if !pinSet {
                Text("Set a parental PIN to lock channels and categories: the PIN is what opens them.")
                    .foregroundStyle(.secondary)
            } else {
                if model.locks.isEmpty {
                    Text("Nothing is locked.").foregroundStyle(.secondary)
                }
                ForEach(model.locks) { lock in
                    HStack {
                        Image(systemName: "lock.fill")
                        Text(lock.label)
                        Text(lock.kind == .channel ? "Channel" : "Category")
                            .font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Button("Unlock") { setLock(lock.kind, lock.value, false) }
                            .accessibilityLabel("Unlock \(lock.label)")
                    }
                }
                Picker("Category to lock", selection: $group) {
                    Text("Choose a category…").tag("")
                    ForEach(groups, id: \.name) { Text("\($0.name) (\($0.count))").tag($0.name) }
                }
                Button("Lock category", systemImage: "lock") {
                    setLock(.category, group, true)
                    group = ""
                }
                .disabled(group.isEmpty)
                TextField("Find a channel to lock", text: $text)
                ForEach(found, id: \.id) { channel in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(channel.name)
                            if let group = channel.group {
                                Text(group).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        Spacer()
                        Button("Lock", systemImage: "lock") {
                            setLock(.channel, String(channel.id), true)
                            text = ""
                        }
                        .accessibilityLabel("Lock \(channel.name)")
                    }
                }
            }
        }
        .task(id: pinSet) {
            if pinSet { groups = await model.playlistGroups(.channels) }
        }
        .task(id: text) {
            let query = text.trimmingCharacters(in: .whitespaces)
            guard query.count >= 2 else { found = []; return }
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else { return }
            let page = await model.playlistPage(
                PlaylistQuery(kind: .channels, text: query, group: nil, show: .visible, duplicatesOnly: false),
                limit: 8, offset: 0)
            found = page?.rows ?? []
        }
    }

    private func setLock(_ kind: ParentalLockKind, _ value: String, _ locked: Bool) {
        run { _ = try $0.parentalSetLock(kind: kind, value: value, locked: locked) }
    }
}

extension ParentalLock: Identifiable {
    public var id: String { "\(kind):\(value)" }
}
