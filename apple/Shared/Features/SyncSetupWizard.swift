import SwiftUI
import UIKit

/// Setting up sync, one decision at a time: Windows' `SyncSetupWizard.tsx`, in its words
/// and its order.
///
///   1. Is this the first device, or is the household already using sync?
///   2. The account: email and password (the service checks these).
///   3. The passphrase (it never leaves the device) -- or, joining, the recovery code.
///   4. This device: its name, whether to share sign-ins, and the privacy policy.
///   5. Creating: the recovery code, which has to be saved before going on.
///   6. Creating: confirm the email address, or leave it for later.
///   7. Done, with what the first sync did and how the next device joins.
///
/// Nothing is sent until step 4. A refusal from the core goes back to the step it is
/// about, so a wrong passphrase reopens the passphrase, not the whole form. The sheet
/// cannot be dismissed while the account is being made or while the recovery code is on
/// screen: closing there would lose the only copy of the code.
struct SyncSetupWizard: View {
    let status: SyncStatus

    enum Path { case create, join }
    enum Step: Hashable { case choose, account, passphrase, device, working, recovery, verify, done }

    /// The steps the progress line shows, per path. `working` and `done` are not steps.
    static func track(_ path: Path) -> [(step: Step, label: String)] {
        switch path {
        case .create:
            [(.account, "Account"), (.passphrase, "Passphrase"), (.device, "This device"),
             (.recovery, "Recovery code"), (.verify, "Email")]
        case .join:
            [(.account, "Account"), (.passphrase, "Passphrase"), (.device, "This device")]
        }
    }

    /// Windows' `MIN_PASSWORD` and the core's `check_passphrase`.
    static let minPassword = 8
    static let minPassphrase = 8

    struct Strength: Equatable {
        enum Tone { case bad, warn, ok }
        let label: String
        let tone: Tone
        let level: Int
    }

    /// Windows' `passphraseStrength`: a rough guide, not a gate beyond the eight
    /// characters the core enforces. Length is what matters, and several words is the
    /// easy way to get it.
    static func strength(_ p: String) -> Strength? {
        if p.isEmpty { return nil }
        let chars = p.unicodeScalars.count
        let words = p.split(whereSeparator: \.isWhitespace).count
        if chars < minPassphrase {
            return Strength(label: "Too short: at least \(minPassphrase) characters", tone: .bad, level: 1)
        }
        if chars >= 20 || (words >= 4 && chars >= 16) { return Strength(label: "Strong", tone: .ok, level: 4) }
        if chars >= 12 || words >= 3 { return Strength(label: "Good", tone: .ok, level: 3) }
        return Strength(label: "Fair: longer is safer. Three or four words is easy to remember", tone: .warn, level: 2)
    }

    /// Windows' `stepForError`: which step a refusal is about, so the viewer lands
    /// where they can fix it.
    static func step(forError message: String) -> Step {
        let m = message.lowercased()
        if m.contains("passphrase") || m.contains("recovery code") { return .passphrase }
        if m.contains("email") || m.contains("password") || m.contains("account") { return .account }
        return .device
    }

    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    @Environment(\.dismiss) private var dismiss

    @State private var path: Path = .create
    @State private var step: Step = .choose
    @State private var email: String
    @State private var password = ""
    @State private var password2 = ""
    @State private var passphrase = ""
    @State private var confirm = ""
    @State private var reveal = false
    @State private var useCode = false
    @State private var code = ""
    @State private var deviceName: String
    @State private var share = false
    @State private var agreed = false
    @State private var error: String?
    @State private var note: String?
    @State private var joined: SyncJoined?
    @State private var shareFailed: String?
    @State private var savedCode = false
    @State private var copied = false
    @State private var verified = false

    init(status: SyncStatus) {
        self.status = status
        _email = State(initialValue: status.email ?? "")
        _deviceName = State(initialValue: status.deviceName ?? UIDevice.current.name)
    }

    private var creating: Bool { path == .create }
    private var locked: Bool { step == .working || step == .recovery }

    // MARK: What each step needs before Continue

    private var trimmedEmail: String { email.trimmingCharacters(in: .whitespaces) }
    private var emailOk: Bool {
        trimmedEmail.range(of: #"\S+@\S+\.\S+"#, options: .regularExpression) != nil
    }
    private var passwordShort: Bool { creating && !password.isEmpty && password.count < Self.minPassword }
    private var passwordMismatch: Bool { creating && !password2.isEmpty && password2 != password }
    private var accountReady: Bool {
        emailOk && !password.isEmpty
            && (!creating || (password.count >= Self.minPassword && password == password2))
    }
    private var samePassphrase: Bool { creating && !passphrase.isEmpty && passphrase == password }
    private var passphraseMismatch: Bool { creating && !confirm.isEmpty && confirm != passphrase }
    private var passphraseReady: Bool {
        if creating {
            return passphrase.unicodeScalars.count >= Self.minPassphrase && passphrase == confirm && !samePassphrase
        }
        return useCode ? !code.trimmingCharacters(in: .whitespaces).isEmpty : !passphrase.isEmpty
    }
    private var deviceReady: Bool {
        !deviceName.trimmingCharacters(in: .whitespaces).isEmpty && (!creating || agreed)
    }

    private var title: String {
        switch step {
        case .choose: "Sync your devices"
        case .account: creating ? "Create your Aurora account" : "Sign in to your household"
        case .passphrase: creating ? "Choose a sync passphrase" : "Unlock your household’s sync"
        case .device: "About this device"
        case .working: creating ? "Creating your account…" : "Joining your household…"
        case .recovery: "Save your recovery code"
        case .verify: "Confirm your email address"
        case .done: "Sync is on"
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Daylight.px(16)) {
                header
                content
                if let note, step != .verify { notice(note, ok: true) }
                if let error, step != .working { notice(error, ok: false) }
            }
            .padding(Daylight.px(24))
            .frame(maxWidth: Daylight.px(560), alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
        .foregroundStyle(theme.text)
        .background(theme.background.ignoresSafeArea())
        .interactiveDismissDisabled(locked)
        #if os(tvOS)
        // Menu is the remote's way out, except where closing would lose the code.
        .onExitCommand { if !locked { dismiss() } }
        #endif
        .animation(.easeOut(duration: 0.2), value: step)
    }

    private func go(_ next: Step) {
        error = nil
        note = nil
        step = next
    }

    // MARK: Header

    private var header: some View {
        HStack(alignment: .top, spacing: Daylight.px(12)) {
            VStack(alignment: .leading, spacing: Daylight.px(8)) {
                progress
                Text(title)
                    .displayFont(.title2)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            #if !os(tvOS)
            if !locked && step != .done {
                Button { dismiss() } label: { Image(systemName: "xmark") }
                    .buttonStyle(.daylight(.ghost, .sm, iconOnly: true))
                    .accessibilityLabel("Close sync setup")
            }
            #endif
        }
    }

    @ViewBuilder
    private var progress: some View {
        let track = Self.track(path)
        if let at = track.firstIndex(where: { $0.step == step }) {
            FlowLayout(spacing: Daylight.px(8)) {
                ForEach(Array(track.enumerated()), id: \.offset) { i, t in
                    Text((i < at ? "✓ " : "") + t.label)
                        .font(.ui(size: Daylight.px(12), weight: .semibold))
                        .padding(.vertical, Daylight.px(3)).padding(.horizontal, Daylight.px(10))
                        .foregroundStyle(i == at ? theme.accentText : i < at ? theme.text : theme.textFaint)
                        .background(i == at ? theme.accent : i < at ? theme.accent.opacity(0.18) : theme.surface,
                                    in: Capsule())
                        .overlay(Capsule().strokeBorder(theme.border, lineWidth: 1))
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Step \(at + 1) of \(track.count)")
        }
    }

    // MARK: Steps

    @ViewBuilder
    private var content: some View {
        switch step {
        case .choose: choose
        case .account: account
        case .passphrase: passphraseStep
        case .device: device
        case .working: working
        case .recovery: recovery
        case .verify: verify
        case .done: done
        }
    }

    private var choose: some View {
        VStack(alignment: .leading, spacing: Daylight.px(12)) {
            VStack(alignment: .leading, spacing: 4) {
                bullet("Favourites, My List, watch progress, profiles, parental settings and recording rules follow you from device to device.")
                bullet("Encrypted on this device before it is uploaded, with a passphrase only your devices know. Not even Aurora can read it.")
                bullet("Free. Playlist passwords and API keys stay on each device unless you choose to share them.")
            }
            choice("This is my first device",
                   "Create a free account for your household. It takes about a minute.") {
                path = .create
                go(.account)
            }
            choice("I already use sync on another device",
                   "Sign in with the household’s email, password and passphrase.") {
                path = .join
                go(.account)
            }
        }
    }

    private var account: some View {
        VStack(alignment: .leading, spacing: Daylight.px(12)) {
            muted(creating
                  ? "The email and password sign each of your devices in. Every other device in the household uses the same ones."
                  : "The email and password your household’s Aurora account was made with.")
            field("Email") {
                TextField("Email", text: $email)
                    .textContentType(.username)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    #endif
            }
            field("Account password") {
                SecureField(creating ? "At least \(Self.minPassword) characters" : "Account password", text: $password)
                    .textContentType(creating ? .newPassword : .password)
            }
            if creating {
                field("Password again") {
                    SecureField("Password again", text: $password2).textContentType(.newPassword)
                }
            }
            if passwordShort { problem("The password needs at least \(Self.minPassword) characters.") }
            if passwordMismatch { problem("The passwords differ.") }
            if !creating {
                Button("Forgot the password?") { forgot() }
                    .buttonStyle(.daylight(.ghost, .sm, wraps: true))
                    .disabled(!emailOk)
            }
            footer(back: .choose, ready: accountReady) { go(.passphrase) }
        }
    }

    private var passphraseStep: some View {
        VStack(alignment: .leading, spacing: Daylight.px(12)) {
            if creating {
                card {
                    Text("Why a second secret?").font(.ui(size: Daylight.px(14), weight: .semibold))
                    muted("The **password** is checked by the sync service. The **passphrase** never leaves your devices: it encrypts everything before it is uploaded, so nobody else can read it. Each new device asks for it once.")
                }
            } else {
                muted("The passphrase that was chosen when the household set sync up. Forgotten it? The recovery code that was saved then works instead.")
            }
            if !creating && useCode {
                field("Recovery code") {
                    TextField("XXXXX-XXXXX-…", text: $code)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.characters)
                        #endif
                }
            } else {
                field("Sync passphrase") {
                    secret(creating ? "For example, four words you will remember" : "Sync passphrase", $passphrase)
                }
            }
            if creating {
                field("Passphrase again") { secret("Passphrase again", $confirm) }
            }
            if !(useCode && !creating) {
                checkbox("Show the passphrase", isOn: $reveal)
            }
            if creating, let strength = Self.strength(passphrase) {
                strengthMeter(strength)
            }
            if passphraseMismatch { problem("The passphrases differ.") }
            if samePassphrase {
                problem("Use a passphrase that is not the account password: the password is checked by the sync service, the passphrase must never reach it.")
            }
            if !creating {
                Button(useCode ? "Use the passphrase instead" : "Forgot the passphrase? Use the recovery code") {
                    useCode.toggle()
                    error = nil
                }
                .buttonStyle(.daylight(.ghost, .sm, wraps: true))
            }
            footer(back: .account, ready: passphraseReady) { go(.device) }
        }
    }

    private var device: some View {
        VStack(alignment: .leading, spacing: Daylight.px(12)) {
            field("Call this device") { TextField("Call this device", text: $deviceName) }
            muted("So you can tell your devices apart. You can sign any of them out later.")
            if creating && status.canShareSecrets {
                checkbox(isOn: $share) {
                    Text("**Also share sign-ins and keys.** Playlist passwords and your TMDB and Gemini keys go to your other devices, encrypted like everything else, so a new device needs only the passphrase. You can change this later.")
                }
            }
            if creating {
                checkbox(isOn: $agreed) {
                    Text("I agree to the privacy policy. Aurora stores your email address and your encrypted library with Google Firebase; it cannot read the library, and you can delete the account and everything in it at any time from Settings.")
                }
                .accessibilityLabel("I agree to the privacy policy")
                #if os(tvOS)
                // A TV has no browser to open it in.
                muted("Privacy policy: github.com/noahklimczuk/iptv-player, apple/AppStore/PRIVACY_POLICY.md")
                #else
                Link("Read the privacy policy", destination: SyncSection.privacyPolicy)
                    .font(.ui(size: Daylight.px(14), weight: .semibold))
                    .foregroundStyle(theme.accent)
                #endif
            }
            footer(back: .passphrase, ready: deviceReady, label: creating ? "Create account" : "Sign in") {
                submit()
            }
        }
    }

    private var working: some View {
        VStack(alignment: .leading, spacing: Daylight.px(8)) {
            ProgressView().tint(theme.accent)
            muted((creating
                   ? "Creating the account, setting up encryption and sending this device’s library."
                   : "Signing in, unlocking the household’s encryption and bringing in your other devices’ changes.")
                  + " This can take a minute on a large library.")
        }
        .accessibilityElement(children: .combine)
    }

    private var recovery: some View {
        VStack(alignment: .leading, spacing: Daylight.px(12)) {
            muted("If the passphrase is ever forgotten, this code is the only way back in. Nobody, Aurora included, can recover it for you. Keep it somewhere safe, away from this device: a password manager, or written down.")
            codeBox(joined?.recoveryCode ?? "")
            #if !os(tvOS)
            Button(copied ? "Copied" : "Copy the code") {
                UIPasteboard.general.string = joined?.recoveryCode
                copied = true
            }
            .buttonStyle(.daylight(.secondary, .sm))
            #endif
            checkbox("I have saved my recovery code somewhere safe", isOn: $savedCode)
            footer(back: nil, ready: savedCode) { go(.verify) }
        }
    }

    private var verify: some View {
        VStack(alignment: .leading, spacing: Daylight.px(12)) {
            muted("We sent a link to **\(trimmedEmail)**. Confirming it is what lets you reset the password if you ever forget it. Sync already works; you can do this later from Settings.")
            FlowLayout(spacing: Daylight.px(8)) {
                Button("I have confirmed it") { recheck() }.buttonStyle(.daylight(.primary))
                Button("Send the email again") { resend() }.buttonStyle(.daylight(.secondary))
                Button("Later") { go(.done) }.buttonStyle(.daylight(.ghost))
            }
            if let note { muted(note) }
        }
    }

    private var done: some View {
        let report = joined?.report
        return VStack(alignment: .leading, spacing: Daylight.px(12)) {
            if joined?.problem != nil {
                notice("This device is signed in. The first sync has not finished yet and will carry on by itself; Settings shows how it is going.", ok: false)
            } else if creating {
                let n = report?.pushed ?? 0
                notice("\(n) \(n == 1 ? "item" : "items") from this device are now in your household’s encrypted sync."
                       + (verified ? " Your email address is confirmed." : ""), ok: true)
            } else {
                let n = report?.applied ?? 0
                notice("Brought in \(n) \(n == 1 ? "change" : "changes") from your other devices."
                       + (verified ? " Your email address is confirmed." : ""), ok: true)
            }
            if let shareFailed {
                notice("Sign-ins and keys are not being shared yet: \(shareFailed). You can turn it on in Settings.", ok: false)
            }
            if creating {
                card {
                    Text("Adding another device").font(.ui(size: Daylight.px(14), weight: .semibold))
                    VStack(alignment: .leading, spacing: 2) {
                        muted("1. Install Aurora on it and open Settings → Account and sync.")
                        muted("2. Choose **Set up sync**, then **I already use sync on another device**.")
                        muted("3. Sign in as \(trimmedEmail) with your account password, then enter the sync passphrase.")
                        #if os(iOS)
                        // Apple's own shortcut: a TV joins from this phone, nothing typed.
                        muted("An Apple TV can join from here instead: Settings → Account and sync → Set up an Apple TV.")
                        #endif
                    }
                }
            }
            HStack {
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(.daylight(.primary))
            }
        }
    }

    // MARK: Calls

    private func forgot() {
        error = nil
        note = nil
        guard let core = model.core else { return }
        let email = trimmedEmail
        Task {
            do {
                try await Task.detached { try core.syncSendPasswordReset(email: email) }.value
                note = "If there is an Aurora account for \(email), an email with a link to choose a new password is on its way. Check the spam folder if it does not arrive."
            } catch {
                self.error = error.viewerMessage
            }
        }
    }

    private func submit() {
        guard let core = model.core else { return }
        error = nil
        step = .working
        let (creating, email, password, passphrase, device, useCode, code) =
            (self.creating, trimmedEmail, self.password, self.passphrase,
             deviceName.trimmingCharacters(in: .whitespaces), self.useCode, self.code)
        let share = creating && self.share && status.canShareSecrets
        Task {
            do {
                let result = try await Task.detached {
                    try creating
                        ? core.syncCreateAccount(email: email, password: password,
                                                 passphrase: passphrase, deviceName: device)
                        : core.syncSignIn(email: email, password: password,
                                          passphrase: useCode ? nil : passphrase,
                                          recoveryCode: useCode ? code : nil,
                                          deviceName: device)
                }.value
                self.password = ""
                self.password2 = ""
                self.confirm = ""
                self.code = ""
                joined = result
                if share {
                    // Its own call, after the account exists: a refusal here is not a
                    // reason to undo an account that worked.
                    do {
                        try await Task.detached { try core.syncSetShareSecrets(enabled: true) }.value
                    } catch {
                        shareFailed = error.viewerMessage
                    }
                }
                self.passphrase = ""
                step = result.recoveryCode != nil ? .recovery : creating ? .verify : .done
            } catch {
                let message = error.viewerMessage
                self.error = message
                step = Self.step(forError: message)
            }
        }
    }

    private func recheck() {
        note = nil
        guard let core = model.core else { return }
        Task {
            do {
                let ok = try await Task.detached { try core.syncEmailVerified() }.value
                verified = ok
                if ok { go(.done) } else { note = "Not confirmed yet. Open the link in the email, then try again." }
            } catch {
                note = error.viewerMessage
            }
        }
    }

    private func resend() {
        note = nil
        guard let core = model.core else { return }
        Task {
            do {
                try await Task.detached { try core.syncSendVerification() }.value
                note = "Sent. Check the spam folder if it does not arrive."
            } catch {
                note = error.viewerMessage
            }
        }
    }

    // MARK: Pieces

    private func muted(_ text: String) -> some View {
        Text(LocalizedStringKey(text))
            .font(.ui(size: Daylight.px(14)))
            .foregroundStyle(theme.textMuted)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func bullet(_ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("•").foregroundStyle(theme.textMuted)
            muted(text)
        }
    }

    private func problem(_ text: String) -> some View {
        Text(text)
            .font(.ui(size: Daylight.px(14)))
            .foregroundStyle(theme.danger)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// Windows' `Message`: a tinted card, green for done and red for a refusal.
    private func notice(_ text: String, ok: Bool) -> some View {
        Text(text)
            .font(.ui(size: Daylight.px(14), weight: .medium))
            .foregroundStyle(ok ? theme.text : theme.danger)
            .fixedSize(horizontal: false, vertical: true)
            .padding(Daylight.px(12))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ok ? theme.success.opacity(0.14) : theme.dangerSoft,
                        in: RoundedRectangle(cornerRadius: Daylight.px(12), style: .continuous))
    }

    private func card<C: View>(@ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 6) { content() }
            .padding(Daylight.px(16))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.surface, in: RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous)
                .strokeBorder(theme.border, lineWidth: 1))
    }

    /// Windows' `Choice`: one of the two first answers, as a card that is a button.
    private func choice(_ title: String, _ body: String, _ pick: @escaping () -> Void) -> some View {
        Button(action: pick) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.ui(size: Daylight.px(16), weight: .bold)).foregroundStyle(theme.text)
                Text(body).font(.ui(size: Daylight.px(14))).foregroundStyle(theme.textMuted)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(Daylight.px(16))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.surface, in: RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Daylight.px(14), style: .continuous)
                .strokeBorder(theme.border, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(14)))
    }

    private func field<C: View>(_ label: String, @ViewBuilder _ control: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.ui(size: Daylight.px(14), weight: .semibold))
            control()
                .font(.ui(size: Daylight.px(16)))
                #if os(iOS)
                .padding(.vertical, 10).padding(.horizontal, 12)
                .background(theme.surface, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(theme.borderStrong, lineWidth: 1))
                #endif
        }
    }

    /// The passphrase, hidden or shown by "Show the passphrase".
    @ViewBuilder
    private func secret(_ prompt: String, _ text: Binding<String>) -> some View {
        if reveal {
            TextField(prompt, text: text)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
        } else {
            SecureField(prompt, text: text)
        }
    }

    private func checkbox(_ label: String, isOn: Binding<Bool>) -> some View {
        checkbox(isOn: isOn) { Text(label) }
    }

    private func checkbox<L: View>(isOn: Binding<Bool>, @ViewBuilder _ label: () -> L) -> some View {
        Button { isOn.wrappedValue.toggle() } label: {
            HStack(alignment: .top, spacing: Daylight.px(10)) {
                DaylightCheckbox(isOn: isOn.wrappedValue)
                label()
                    .font(.ui(size: Daylight.px(14)))
                    .foregroundStyle(theme.textMuted)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(FocusRingStyle(radius: Daylight.px(8)))
        .accessibilityAddTraits(isOn.wrappedValue ? [.isSelected] : [])
    }

    private func strengthMeter(_ s: Strength) -> some View {
        let tone: Color = switch s.tone {
        case .ok: theme.success
        case .warn: .orange
        case .bad: theme.danger
        }
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 4) {
                ForEach(1...4, id: \.self) { n in
                    Capsule().fill(n <= s.level ? tone : theme.surfaceSunk).frame(height: 4)
                }
            }
            .accessibilityHidden(true)
            Text("Passphrase strength: \(s.label)")
                .font(.ui(size: Daylight.px(14), weight: .semibold))
                .foregroundStyle(tone)
        }
        .animation(.easeOut(duration: 0.15), value: s)
    }

    private func codeBox(_ code: String) -> some View {
        Text(code)
            .font(.system(size: Daylight.px(17), design: .monospaced))
            .tracking(0.6)
            #if os(iOS)
            .textSelection(.enabled)
            #endif
            .padding(Daylight.px(12))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: Daylight.px(10), style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Daylight.px(10), style: .continuous)
                .strokeBorder(theme.border, lineWidth: 1))
    }

    /// Windows' `Footer`: Back on the left, the step's Continue on the right.
    private func footer(back: Step?, ready: Bool, label: String = "Continue",
                        _ next: @escaping () -> Void) -> some View {
        HStack {
            if let back {
                Button("Back") { go(back) }.buttonStyle(.daylight(.ghost))
            }
            Spacer()
            Button(label, action: next)
                .buttonStyle(.daylight(.primary))
                .disabled(!ready)
        }
        .padding(.top, 4)
    }
}
