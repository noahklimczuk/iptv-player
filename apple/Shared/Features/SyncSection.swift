import SwiftUI
import UIKit

/// Settings → Account and sync: Windows' Sync panel (`SyncPanel.tsx`), same wording.
///
/// Every decision is in the Rust core (`aurora-ffi/src/sync.rs` over
/// `aurora_sync::household`); this only shows its status and passes on what was typed.
/// Each call blocks on the network, so it runs off the main actor.
struct SyncSection: View {
    /// Its heading, which the setup wizard's offer of an account opens Settings at.
    static let title = "Account and sync"
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
    @State private var confirmPassword = ""
    @State private var agreed = false
    @State private var passphrase = ""
    @State private var confirm = ""
    @State private var code = ""
    @State private var deviceName = UIDevice.current.name
    @State private var showCodeFor = ""
    /// The account password, typed again to delete the account.
    @State private var deletePassword = ""
    /// Whether the account's email address is confirmed: asked once per sign-in, not on
    /// every reload (an account call, as Windows' panel does). `nil` until known.
    @State private var verified: Bool?
    @State private var verifyNote: String?

    private enum Confirm { case signOut, reset, deleteAccount }
    /// Pairing (`Pairing.swift`): the TV showing its code, the phone setting a TV up.
    @State private var pairing = false

    var body: some View {
        Section {
            SettingsHead(Self.title).id(Self.title)
            if let status {
                content(status)
            } else {
                ProgressView()
            }
            if let message {
                Text(message).font(.ui(.caption)).foregroundStyle(.secondary)
            }
        }
        .task { await reload() }
        .task(id: status?.signedIn == true ? status?.email : nil) { await checkVerified() }
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
                .font(.ui(.caption)).foregroundStyle(.secondary)
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
        if verified == false {
            VStack(alignment: .leading, spacing: 6) {
                Text("Confirm your email address").fontWeight(.heavy)
                Text("We sent a link to \(status.email ?? "your address"). Confirming it is what lets you reset the password if you ever forget it. Sync works in the meantime.")
                    .font(.ui(.caption)).foregroundStyle(.secondary)
                HStack {
                    Button("I have confirmed it") { Task { await checkVerified(saying: true) } }
                    Button("Send the email again") { resendVerification() }
                }
                .disabled(busy)
                if let verifyNote {
                    Text(verifyNote).font(.ui(.caption)).foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 4)
            .overlay(alignment: .leading) {
                Rectangle().fill(.orange).frame(width: 3).offset(x: -10)
            }
        }
        VStack(alignment: .leading, spacing: 4) {
            Text(status.email ?? "")
            if let name = status.deviceName {
                Text("on \(name)").font(.ui(.caption)).foregroundStyle(.secondary)
            }
            let state = Self.describe(status)
            Text(state.text).font(.ui(.caption)).foregroundStyle(state.color)
            if status.parked > 0 {
                Text("\(status.parked) \(status.parked == 1 ? "change refers" : "changes refer") to channels or titles this device has not imported. They apply once it has.")
                    .font(.ui(.caption)).foregroundStyle(.secondary)
            }
            // Skipped, never applied, and asked for again by the update that can read
            // them, which is the one thing the viewer can do about it (Windows' §3.8).
            if status.unreadable > 0 {
                let one = status.unreadable == 1
                Text("\(status.unreadable) \(one ? "change" : "changes") from your other devices \(one ? "needs" : "need") a newer version of Aurora. \(one ? "It is" : "They are") kept on the server and will be read once this device updates.")
                    .font(.ui(.caption)).foregroundStyle(.secondary)
            }
            Text("Today this device has read \(status.readsToday.formatted()) and written \(status.writesToday.formatted()) of the free plan's 50,000 reads and 20,000 writes.")
                .font(.ui(.caption)).foregroundStyle(.secondary)
            if status.thrifty {
                Text("That is past this device's share, so until the quota resets at midnight Pacific it sends watch progress and checks for changes every ten minutes. Favourites and other edits still go straight away.")
                    .font(.ui(.caption)).foregroundStyle(.orange)
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
                        .font(.ui(.caption)).foregroundStyle(.secondary)
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
                .font(.ui(.caption)).foregroundStyle(.red)
            Button("Yes, sign out", role: .destructive) {
                run("Could not sign out") { core in
                    try core.syncSignOut()
                    return "Signed out. This device keeps everything it has."
                }
            }
            Button("Keep") { confirming = nil }
        case .some(.reset):
            Text("Replace everything the household has synced with this device's library? Changes other devices have not sent yet are lost.")
                .font(.ui(.caption)).foregroundStyle(.red)
            Button("Yes, reset", role: .destructive) {
                run("Could not reset sync") { core in
                    let r = try core.syncReset()
                    return "Sync reset from this device: \(r.pushed) records uploaded."
                }
            }
            Button("Keep") { confirming = nil }
        case .some(.deleteAccount):
            // Behind the password: the one action here that cannot be undone, and it
            // reaches every device in the household (Windows' delete form).
            Text("Delete your Aurora account?").fontWeight(.heavy).foregroundStyle(.red)
            Text("This permanently deletes \(status.email ?? "the account") and everything the household has synced to it: favourites, lists, watch progress, profiles and settings on the server. The library on each device stays as it is, but no device will sync again until someone creates a new account. This cannot be undone.")
                .font(.ui(.caption)).foregroundStyle(.secondary)
            SecureField("Account password", text: $deletePassword)
                .textContentType(.password)
            Button(busy ? "Deleting…" : "Delete account permanently", role: .destructive) {
                let typed = deletePassword
                deletePassword = ""
                run("Could not delete the account") { core in
                    try core.syncDeleteAccount(password: typed)
                    return "Your Aurora account and everything it stored have been deleted. This device keeps its library; other devices stop syncing."
                }
            }
            .disabled(busy || deletePassword.isEmpty)
            Button("Keep it") { confirming = nil; deletePassword = "" }
        case nil:
            Button("Sign out", role: .destructive) { confirming = .signOut }
            Button("Reset sync from this device") { confirming = .reset }
            Button("Delete account", role: .destructive) { confirming = .deleteAccount }
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

    // MARK: Creating an account and signing in

    /// Where the privacy policy is published: this repository's copy, which the App
    /// Store listing links to as well (AppStore/PRIVACY_POLICY.md).
    static let privacyPolicy = URL(string: "https://github.com/noahklimczuk/iptv-player/blob/main/apple/AppStore/PRIVACY_POLICY.md")!
    /// Windows' `MIN_PASSWORD`: stricter than Firebase's six, since an account guards
    /// a household's sync.
    private static let minPassword = 8
    /// The core's `check_passphrase`.
    private static let minPassphrase = 8

    /// Windows' `SignInForm`, in its words: Create account and Sign in, the password
    /// twice, agreement to the privacy policy, and "Forgot the password?".
    @ViewBuilder
    private func form(_ status: SyncStatus) -> some View {
        let again = status.signedIn && status.problem == .signInAgain
        let creating = !joining && !again
        if !again {
            Segments(options: [(false, "Create account"), (true, "Sign in")], selection: $joining)
        }
        Text(again
             ? "The sync account needs its password again. Nothing on this device has been lost."
             : creating
             ? "A free Aurora account, for the first device in your household. Every other device then signs in with the same email, password and passphrase."
             : "For a device joining a household that already has an Aurora account.")
            .font(.ui(.caption)).foregroundStyle(.secondary)
        #if os(tvOS)
        // Typing with a remote is slow: a phone that already syncs can do it.
        if creating {
            Text("Easier on an iPhone or iPad: create the account in Aurora there, then choose Sign in here and Sign in with your iPhone.")
                .font(.ui(.caption)).foregroundStyle(.secondary)
        } else {
            Button("Sign in with your iPhone", systemImage: "iphone") { pairing = true }
                .disabled(busy)
        }
        #endif
        TextField("Email", text: $email)
            .textContentType(.username)
            .autocorrectionDisabled()
            #if os(iOS)
            .keyboardType(.emailAddress)
            .textInputAutocapitalization(.never)
            #endif
        SecureField(creating ? "Account password (at least \(Self.minPassword) characters)" : "Account password", text: $password)
            .textContentType(creating ? .newPassword : .password)
        if creating {
            SecureField("Password again", text: $confirmPassword)
                .textContentType(.newPassword)
            if !password.isEmpty && password.count < Self.minPassword {
                hint("The password needs at least \(Self.minPassword) characters.")
            }
            if !confirmPassword.isEmpty && confirmPassword != password {
                hint("The passwords differ.")
            }
        } else if !again {
            Button("Forgot the password?") { sendPasswordReset() }
                .disabled(busy || !email.contains("@"))
        }
        if !creating && useCode {
            TextField("Recovery code", text: $code)
                .autocorrectionDisabled()
        } else {
            SecureField(creating ? "Sync passphrase (at least \(Self.minPassphrase) characters)" : "Sync passphrase", text: $passphrase)
        }
        if creating {
            SecureField("Passphrase again", text: $confirm)
            Text("Different from the password, and it never leaves your devices: it is what encrypts everything before it is uploaded. Each device asks for it once.")
                .font(.ui(.caption)).foregroundStyle(.secondary)
            if !confirm.isEmpty && confirm != passphrase {
                hint("The passphrases differ.")
            }
            if !passphrase.isEmpty && passphrase == password {
                hint("Use a passphrase that is not the account password: the password is checked by the sync service, the passphrase must never reach it.")
            }
        }
        if !creating && !again {
            Button(useCode ? "Use the passphrase instead" : "Forgot the passphrase? Use the recovery code") {
                useCode.toggle()
            }
        }
        TextField("Call this device", text: $deviceName)
        if creating {
            agreement
        }
        Button(busy ? (creating ? "Creating account…" : "Signing in…")
               : again ? "Sign in again" : creating ? "Create account" : "Sign in") {
            submit(joining: !creating)
        }
        .disabled(busy || !formReady(joining: !creating))
    }

    /// Windows' "I agree to the privacy policy" box, which Create account waits for.
    @ViewBuilder
    private var agreement: some View {
        Toggle(isOn: $agreed) {
            Text("I agree to the privacy policy. Aurora stores your email address and your encrypted library with Google Firebase; it cannot read the library, and you can delete the account and everything in it at any time from this page.")
                .font(.ui(.caption))
        }
        .accessibilityLabel("I agree to the privacy policy")
        #if os(tvOS)
        // A TV has no browser to open it in.
        Text("Privacy policy: github.com/noahklimczuk/iptv-player, apple/AppStore/PRIVACY_POLICY.md")
            .font(.ui(.caption)).foregroundStyle(.secondary)
        #else
        Link("Read the privacy policy", destination: Self.privacyPolicy)
            .font(.ui(.caption))
        #endif
    }

    private func hint(_ text: String) -> some View {
        Text(text).font(.ui(.caption)).foregroundStyle(.red)
    }

    private func formReady(joining: Bool) -> Bool {
        guard !email.isEmpty, !password.isEmpty, !deviceName.isEmpty else { return false }
        if joining { return useCode ? !code.isEmpty : !passphrase.isEmpty }
        return password.count >= Self.minPassword && password == confirmPassword && agreed
            && passphrase.count >= Self.minPassphrase && passphrase == confirm
            && passphrase != password
    }

    private func submit(joining: Bool) {
        let (email, password, passphrase, code, device, useCode) =
            (self.email.trimmingCharacters(in: .whitespaces), self.password, self.passphrase,
             self.code, self.deviceName, self.useCode)
        run(joining ? "Could not sign in" : "Could not create the account") { core in
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
        self.confirmPassword = ""
        self.passphrase = ""
        self.confirm = ""
        self.code = ""
    }

    /// Windows' `recheck`: asks the account whether its address is confirmed. With
    /// `saying`, a "not yet" is said, for the button that asked.
    private func checkVerified(saying: Bool = false) async {
        verifyNote = nil
        guard let core = model.core, status?.signedIn == true else {
            verified = nil
            return
        }
        do {
            let ok = try await Task.detached { try core.syncEmailVerified() }.value
            verified = ok
            if !ok && saying {
                verifyNote = "Not confirmed yet. Open the link in the email, then try again."
            }
        } catch {
            if saying { verifyNote = error.viewerMessage } else { verified = nil }
        }
    }

    private func resendVerification() {
        guard let core = model.core else { return }
        busy = true
        Task {
            do {
                try await Task.detached { try core.syncSendVerification() }.value
                verifyNote = "Sent. Check the spam folder if it does not arrive."
            } catch {
                verifyNote = "Could not send the email: \(error.viewerMessage)"
            }
            busy = false
        }
    }

    private func sendPasswordReset() {
        let email = self.email.trimmingCharacters(in: .whitespaces)
        run("Could not send the email") { core in
            try core.syncSendPasswordReset(email: email)
            return "If there is an Aurora account for \(email), an email with a link to choose a new password is on its way. Check the spam folder if it does not arrive."
        }
    }

    // MARK: The recovery code

    @ViewBuilder
    private func recovery(_ code: String) -> some View {
        Text("Your recovery code").font(.ui(.headline))
        Text("Write this down and keep it somewhere safe. If the sync passphrase is ever forgotten, this is the only way back in: nobody, Aurora included, can recover it for you.")
            .font(.ui(.caption)).foregroundStyle(.secondary)
        #if os(iOS)
        Text(code).font(.ui(.body).monospaced()).textSelection(.enabled)
        #else
        Text(code).font(.ui(.body).monospaced())
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
