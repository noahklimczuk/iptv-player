import SwiftUI
import UIKit

/// Settings → Sync: Windows' Sync panel (`SyncPanel.tsx`), same wording.
///
/// Every decision is in the Rust core (`aurora-ffi/src/sync.rs` over
/// `aurora_sync::household`); this only shows its status and passes on what was typed.
/// Each call blocks on the network, so it runs off the main actor.
struct SyncSection: View {
    @Environment(AppModel.self) private var model
    @State private var status: SyncStatus?
    @State private var busy = false
    @State private var message: String?
    @State private var recoveryCode: String?
    @State private var confirming: Confirm?

    // The form.
    @State private var joining = false
    @State private var useCode = false
    @State private var email = ""
    @State private var password = ""
    @State private var passphrase = ""
    @State private var confirm = ""
    @State private var code = ""
    @State private var deviceName = UIDevice.current.name
    @State private var showCodeFor = ""

    private enum Confirm { case signOut, reset }
    /// Pairing (`Pairing.swift`): the TV showing its code, the phone setting a TV up.
    @State private var pairing = false

    var body: some View {
        Section {
            SettingsHead("Sync").id("Sync")
            if let status {
                content(status)
            } else {
                ProgressView()
            }
            if let message {
                Text(message).font(.caption).foregroundStyle(.secondary)
            }
        }
        .task { await reload() }
        #if os(tvOS)
        .fullScreenCover(isPresented: $pairing) {
            PairWithPhoneView(deviceName: deviceName) { Task { await reload() } }
                .auroraAppearance()
        }
        #else
        .sheet(isPresented: $pairing) {
            SetUpAppleTVView(email: status?.email ?? email).auroraAppearance()
        }
        #endif
    }

    @ViewBuilder
    private func content(_ status: SyncStatus) -> some View {
        if !status.available {
            Text("This build of Aurora was made without a sync service, so there is nothing to sign in to. Official releases include it.")
                .font(.caption).foregroundStyle(.secondary)
        } else if let recoveryCode {
            recovery(recoveryCode)
        } else if !status.signedIn || status.problem == .signInAgain {
            form(status)
        } else {
            signedIn(status)
        }
    }

    // MARK: Signed in

    @ViewBuilder
    private func signedIn(_ status: SyncStatus) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(status.email ?? "")
            if let name = status.deviceName {
                Text("on \(name)").font(.caption).foregroundStyle(.secondary)
            }
            let state = Self.describe(status)
            Text(state.text).font(.caption).foregroundStyle(state.color)
            if status.parked > 0 {
                Text("\(status.parked) \(status.parked == 1 ? "change refers" : "changes refer") to channels or titles this device has not imported. They apply once it has.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            // Skipped, never applied, and asked for again by the update that can read
            // them, which is the one thing the viewer can do about it (Windows' §3.8).
            if status.unreadable > 0 {
                let one = status.unreadable == 1
                Text("\(status.unreadable) \(one ? "change" : "changes") from your other devices \(one ? "needs" : "need") a newer version of Aurora. \(one ? "It is" : "They are") kept on the server and will be read once this device updates.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Text("Today this device has read \(status.readsToday.formatted()) and written \(status.writesToday.formatted()) of the free plan's 50,000 reads and 20,000 writes.")
                .font(.caption).foregroundStyle(.secondary)
            if status.thrifty {
                Text("That is past this device's share, so until the quota resets at midnight Pacific it sends watch progress and checks for changes every ten minutes. Favourites and other edits still go straight away.")
                    .font(.caption).foregroundStyle(.orange)
            }
        }
        // D34: a password is a different promise from a favourite, so this is a choice
        // somebody makes, off until they do, and it says what it means both ways.
        if status.canShareSecrets {
            Toggle(isOn: Binding(get: { status.sharesSecrets }, set: shareSecrets)) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Also share sign-ins and keys").fontWeight(.heavy)
                    Text(status.sharesSecrets
                         ? "Playlist passwords and your TMDB and Gemini keys go to the household’s other devices, encrypted like everything else, so a new device needs only the sync passphrase. Turning this off stops sending them; what was sent stays in the household’s encrypted sync until you reset it."
                         : "Off: each device keeps its own playlist passwords and TMDB and Gemini keys. On sends them to your other devices, encrypted with your sync passphrase, so nobody has to type them twice.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            .disabled(busy)
            .accessibilityLabel(status.sharesSecrets ? "Stop sharing sign-ins and keys" : "Share sign-ins and keys")
        }
        #if os(iOS)
        // An Apple TV joins from here, so nothing is typed with its remote.
        Button("Set up an Apple TV", systemImage: "appletv") { pairing = true }
            .disabled(busy)
        #endif
        Button(busy ? "Syncing…" : "Sync now", systemImage: "arrow.triangle.2.circlepath") {
            run("Sync failed") { core in
                let r = try core.syncNow()
                return r.pushed + r.pulled == 0
                    ? "Already up to date."
                    : "Sent \(r.pushed), received \(r.pulled)."
            }
        }
        .disabled(busy)
        SecureField("Sync passphrase, to show the recovery code", text: $showCodeFor)
            .textContentType(.password)
            .onSubmit { showRecoveryCode() }
        if !showCodeFor.isEmpty {
            Button("Show the recovery code") { showRecoveryCode() }.disabled(busy)
        }
        switch confirming {
        case .some(.signOut):
            Text("Stop syncing on this device? Its library stays as it is.")
                .font(.caption).foregroundStyle(.red)
            Button("Yes, sign out", role: .destructive) {
                run("Could not sign out") { core in
                    try core.syncSignOut()
                    return "Signed out. This device keeps everything it has."
                }
            }
            Button("Keep") { confirming = nil }
        case .some(.reset):
            Text("Replace everything the household has synced with this device's library? Changes other devices have not sent yet are lost.")
                .font(.caption).foregroundStyle(.red)
            Button("Yes, reset", role: .destructive) {
                run("Could not reset sync") { core in
                    let r = try core.syncReset()
                    return "Sync reset from this device: \(r.pushed) records uploaded."
                }
            }
            Button("Keep") { confirming = nil }
        case nil:
            Button("Sign out", role: .destructive) { confirming = .signOut }
            Button("Reset sync from this device…") { confirming = .reset }
        }
    }

    /// Windows' `describe`: one line, coloured by how much it matters.
    private static func describe(_ s: SyncStatus) -> (text: String, color: Color) {
        let waiting = s.pending == 1 ? "1 change waiting" : "\(s.pending) changes waiting"
        switch s.problem {
        case .some(.offline):
            return ("Offline — \(waiting)", .orange)
        case .some(.quota):
            return ("The sync service's free daily allowance is used up. It resets at midnight Pacific time — \(waiting).", .orange)
        case .some(.signInAgain):
            return ("Sync has stopped: sign in again to carry on.", .red)
        case .some(.failed(let message)):
            return (message, .red)
        case nil:
            break
        }
        if !s.joined { return ("Finishing the first sync…", .orange) }
        if s.pending > 0 { return ("Syncing — \(waiting)", .secondary) }
        let ago = s.lastSyncAt.map {
            Date(timeIntervalSince1970: TimeInterval($0)).formatted(.relative(presentation: .named))
        } ?? "never"
        return ("Up to date. Last synced \(ago).", .secondary)
    }

    // MARK: Setting up and joining

    @ViewBuilder
    private func form(_ status: SyncStatus) -> some View {
        let again = status.signedIn && status.problem == .signInAgain
        Text("Keeps favourites, My List, watch progress, profiles, parental settings and recording rules the same on every device in the household. Everything is encrypted on this device before it is uploaded, with a passphrase only your devices know. Playlist passwords and API keys stay on each device unless you choose to share them too.")
            .font(.caption).foregroundStyle(.secondary)
        if !again {
            Segments(options: [(false, "Set up sync"), (true, "Join my household")], selection: $joining)
        }
        Text(again
             ? "The sync account needs its password again. Nothing on this device has been lost."
             : joining
                ? "For a device joining a household that already set sync up on another one."
                : "For the first device in the household. Every other device then joins with the same email, password and passphrase.")
            .font(.caption).foregroundStyle(.secondary)
        let joiningNow = joining || again
        #if os(tvOS)
        // Joining from a remote is a lot of typing: a phone that already syncs can do it.
        if joiningNow {
            Button("Sign in with your iPhone", systemImage: "iphone") { pairing = true }
                .disabled(busy)
        }
        #endif
        TextField("Email", text: $email)
            .textContentType(.emailAddress)
            .autocorrectionDisabled()
            #if os(iOS)
            .keyboardType(.emailAddress)
            .textInputAutocapitalization(.never)
            #endif
        SecureField("Account password", text: $password)
            .textContentType(.password)
        if joiningNow && useCode {
            TextField("Recovery code", text: $code)
                .autocorrectionDisabled()
        } else {
            SecureField(joiningNow ? "Sync passphrase" : "Sync passphrase (at least 8 characters)", text: $passphrase)
            if !joiningNow {
                SecureField("Passphrase again", text: $confirm)
                if !confirm.isEmpty && confirm != passphrase {
                    Text("The passphrases differ.").font(.caption).foregroundStyle(.red)
                }
            }
        }
        if joiningNow {
            Button(useCode ? "Use the passphrase instead" : "Forgot the passphrase? Use the recovery code") {
                useCode.toggle()
            }
        }
        TextField("Call this device", text: $deviceName)
        Button(busy ? (joiningNow ? "Signing in…" : "Setting up…")
               : again ? "Sign in again" : joiningNow ? "Join" : "Set up sync") {
            submit(joining: joiningNow)
        }
        .disabled(busy || !formReady(joining: joiningNow))
    }

    private func formReady(joining: Bool) -> Bool {
        guard !email.isEmpty, !password.isEmpty, !deviceName.isEmpty else { return false }
        if joining { return useCode ? !code.isEmpty : !passphrase.isEmpty }
        return passphrase.count >= 8 && passphrase == confirm
    }

    private func submit(joining: Bool) {
        let (email, password, passphrase, code, device, useCode) =
            (self.email, self.password, self.passphrase, self.code, self.deviceName, self.useCode)
        run(joining ? "Could not sign in" : "Could not set up sync") { core in
            let joined = joining
                ? try core.syncSignIn(email: email, password: password,
                                      passphrase: useCode ? nil : passphrase,
                                      recoveryCode: useCode ? code : nil,
                                      deviceName: device)
                : try core.syncCreateAccount(email: email, password: password,
                                             passphrase: passphrase, deviceName: device)
            return joined.recoveryCode.map { "code:\($0)" }
                ?? (joined.problem == nil ? "Signed in." : "Signed in. The first sync will finish when the service can be reached.")
        }
        self.password = ""
        self.passphrase = ""
        self.confirm = ""
        self.code = ""
    }

    // MARK: The recovery code

    @ViewBuilder
    private func recovery(_ code: String) -> some View {
        Text("Your recovery code").font(.headline)
        Text("Write this down and keep it somewhere safe. If the sync passphrase is ever forgotten, this is the only way back in: nobody, Aurora included, can recover it for you.")
            .font(.caption).foregroundStyle(.secondary)
        #if os(iOS)
        Text(code).font(.body.monospaced()).textSelection(.enabled)
        #else
        Text(code).font(.body.monospaced())
        #endif
        Button("I have written it down") {
            recoveryCode = nil
            Task { await reload() }
        }
    }

    private func showRecoveryCode() {
        let passphrase = showCodeFor
        guard !passphrase.isEmpty, let core = model.core else { return }
        busy = true
        Task {
            do {
                let code = try await Task.detached { try core.syncRecoveryCode(passphrase: passphrase) }.value
                showCodeFor = ""
                recoveryCode = code
            } catch {
                message = "Could not show the recovery code: \(error.viewerMessage)"
            }
            busy = false
        }
    }

    // MARK: Plumbing

    /// Windows' `sync.setShareSecrets`, with its two messages.
    private func shareSecrets(_ on: Bool) {
        run("Could not change that") { core in
            try core.syncSetShareSecrets(enabled: on)
            return on
                ? "Sign-ins and keys will go to your other devices from the next sync."
                : "Sign-ins and keys are no longer sent."
        }
    }

    /// Run a core call off the main actor, then show what it said and read the status
    /// again. A result starting `code:` is a recovery code to show.
    private func run(_ failure: String, _ body: @escaping @Sendable (AuroraCore) throws -> String) {
        guard let core = model.core else { return }
        busy = true
        message = nil
        confirming = nil
        Task {
            do {
                let said = try await Task.detached { try body(core) }.value
                if said.hasPrefix("code:") {
                    recoveryCode = String(said.dropFirst(5))
                } else {
                    message = said
                }
            } catch {
                message = "\(failure): \(error.viewerMessage)"
            }
            busy = false
            await reload()
        }
    }

    private func reload() async {
        guard let core = model.core else { return }
        do {
            status = try await Task.detached { try core.syncStatus() }.value
            if let email = status?.email, self.email.isEmpty { self.email = email }
        } catch {
            message = "Could not read the sync status: \(error.viewerMessage)"
        }
    }
}

extension AppModel {
    /// The sync loop, while a scene is active (`RootView`). The core's pacer decides
    /// whether a round is due; this only asks every few seconds, as Windows' loop does.
    /// iOS suspends the app in the background, so the loop simply stops there and the
    /// next activation starts it again.
    func runSyncLoop() async {
        guard let core else { return }
        let seconds = core.syncTickSecs()
        while !Task.isCancelled {
            await Task.detached(priority: .utility) { core.syncTick() }.value
            try? await Task.sleep(for: .seconds(Double(seconds)))
        }
    }
}
