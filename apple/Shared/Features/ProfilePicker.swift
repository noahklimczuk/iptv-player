import SwiftUI

/// "Who's watching?", the PIN pad for a profile that has one, and adding, editing and
/// deleting profiles. Windows' `ProfilePicker.tsx` (#166): four digits submit by
/// themselves, and a wrong PIN says how many tries are left. The attempt count and the lockout are the shared database's
/// (`profiles::verify_profile_pin`), so they are the same tries Windows counts.
struct ProfilePicker: View {
    @Environment(AppModel.self) private var model
    @State private var unlocking: Profile?
    /// "Manage profiles": a tile opens the editor instead of choosing.
    @State private var managing = false
    /// The editor, for a profile or (`.new`) for a new one.
    @State private var editing: EditTarget?

    enum EditTarget: Identifiable, Hashable {
        case new
        case existing(Profile)
        var id: String {
            switch self {
            case .new: "new"
            case .existing(let p): "profile-\(p.id)"
            }
        }
    }

    var body: some View {
        VStack(spacing: 40) {
            Text(managing ? "Manage profiles" : "Who's watching?").font(.largeTitle.bold())
            ScrollView(.horizontal) {
                HStack(spacing: 32) {
                    ForEach(model.profiles, id: \.id) { profile in
                        Button {
                            choose(profile)
                        } label: {
                            ProfileTile(profile: profile, editing: managing).padding()
                        }
                        .tileButtonStyle()
                    }
                    if !managing {
                        Button { editing = .new } label: { AddTile().padding() }
                            .tileButtonStyle()
                    }
                }
                .padding()
            }
            .scrollClipDisabled()
            HStack(spacing: 24) {
                Button(managing ? "Done" : "Manage profiles", systemImage: managing ? "checkmark" : "pencil") {
                    managing.toggle()
                }
                // Back to what was being watched, when there is something to go back to:
                // the picker reached from Settings, not the one at launch.
                if model.activeProfile != nil {
                    Button("Back", systemImage: "chevron.backward") {
                        managing = false
                        model.picking = false
                    }
                }
            }
        }
        .padding()
        // Fresh each time it opens: the editor may have added, renamed or deleted one.
        .task { await model.loadProfiles() }
        .sheet(item: $unlocking) { profile in
            // Checked by the core as it chooses the profile, not by this screen: what a
            // profile may watch is decided there, so who is watching must be too.
            PinPad(heading: "Enter \(profile.name)'s PIN") { pin in
                await model.choose(profile, pin: pin)
            } onUnlocked: {
                unlocking = nil
            }
            .auroraAppearance()
        }
        // Adding or removing a profile needs the parental PIN once one is set.
        .parentalPinHost()
        .sheet(item: $editing) { target in
            ProfileEditor(target: target, canDelete: model.profiles.count > 1)
                .parentalPinHost()
                .auroraAppearance()
        }
    }

    private func choose(_ profile: Profile) {
        if managing {
            editing = .existing(profile)
        } else if profile.hasPin {
            unlocking = profile
        } else {
            Task { await model.choose(profile) }
        }
    }
}

/// Add or change a profile, as Windows' editor does: a name, Kids (when adding: a kids
/// profile starts limited to age 12), a four-digit PIN to set, change or remove, and
/// Delete (never the last profile). The rules are enforced in the core; the form only
/// stops a save it knows will be refused.
private struct ProfileEditor: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let target: ProfilePicker.EditTarget
    let canDelete: Bool

    @State private var name = ""
    @State private var kids = false
    @State private var pin = ""
    @State private var removePin = false
    @State private var confirmingDelete = false
    @State private var busy = false
    @State private var message: String?

    private var existing: Profile? {
        if case .existing(let p) = target { return p }
        return nil
    }
    private static func isDigit(_ c: Character) -> Bool { c.isASCII && c.isNumber }
    private var trimmed: String { name.trimmingCharacters(in: .whitespaces) }
    private var pinOK: Bool { pin.isEmpty || (pin.count == 4 && pin.allSatisfy(Self.isDigit)) }
    private var canSave: Bool { !trimmed.isEmpty && pinOK && !busy }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $name)
                if existing == nil {
                    Toggle("Kids profile", isOn: $kids)
                }
                Section {
                    SecureField(existing?.hasPin == true ? "New PIN (4 digits)" : "PIN (4 digits, optional)", text: $pin)
                        #if os(iOS)
                        .keyboardType(.numberPad)
                        #endif
                        .onChange(of: pin) { pin = String(pin.filter(Self.isDigit).prefix(4)) }
                    if existing?.hasPin == true && pin.isEmpty {
                        Toggle("Remove the PIN", isOn: $removePin)
                    }
                } footer: {
                    if !pinOK { Text("A PIN is 4 digits.") }
                }
                if let message {
                    Text(message).foregroundStyle(.orange)
                }
                if existing != nil && canDelete {
                    Button("Delete profile", role: .destructive) { confirmingDelete = true }
                }
            }
            .auroraPageTitle(existing == nil ? "Add profile" : "Edit \(existing!.name)")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { Task { await save() } }.disabled(!canSave)
                }
            }
            .confirmationDialog("Delete \(existing?.name ?? "")?", isPresented: $confirmingDelete,
                                titleVisibility: .visible) {
                Button("Delete", role: .destructive) { Task { await remove() } }
            } message: {
                Text("Its favourites, My List and watch history go with it.")
            }
        }
        .onAppear { name = existing?.name ?? "" }
    }

    private func save() async {
        busy = true
        defer { busy = false }
        model.error = nil
        let ok: Bool
        if let existing {
            let change: PinChange = !pin.isEmpty ? .set(pin: pin) : removePin ? .remove : .keep
            ok = await model.updateProfile(existing.id, name: trimmed, pin: change)
        } else {
            ok = await model.createProfile(name: trimmed, isKids: kids, pin: pin.isEmpty ? nil : pin)
        }
        if ok { dismiss() } else { message = model.error; model.error = nil }
    }

    private func remove() async {
        guard let existing else { return }
        busy = true
        defer { busy = false }
        model.error = nil
        if await model.deleteProfile(existing.id) { dismiss() } else { message = model.error; model.error = nil }
    }
}

private struct AddTile: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "plus")
                .font(.system(size: 48, weight: .bold))
                .frame(width: 140, height: 140)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 40))
            Text("Add profile").font(.headline)
            Text(" ").font(.caption)
        }
    }
}

extension Profile: Identifiable {}

private struct ProfileTile: View {
    let profile: Profile
    var editing = false

    var body: some View {
        VStack(spacing: 12) {
            Text(profile.name.prefix(1).uppercased())
                .font(.system(size: 56, weight: .heavy, design: .rounded))
                .frame(width: 140, height: 140)
                .background(Self.colour(profile), in: RoundedRectangle(cornerRadius: 40))
                .foregroundStyle(.white)
                .overlay(alignment: .bottomTrailing) {
                    if editing {
                        Image(systemName: "pencil.circle.fill").font(.title).padding(6)
                    }
                }
            Text(profile.name).font(.headline)
            HStack(spacing: 6) {
                if profile.hasPin { Label("PIN", systemImage: "lock.fill") }
                if profile.isKids { Text("KIDS").foregroundStyle(.green) }
            }
            .font(.caption)
        }
    }

    /// A steady colour per profile, so the same person is the same colour every time.
    private static func colour(_ p: Profile) -> Color {
        let hues: [Color] = [.indigo, .teal, .orange, .pink, .purple, .blue, .green]
        return hues[Int(p.id.magnitude % UInt64(hues.count))]
    }
}
