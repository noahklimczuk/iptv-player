import SwiftUI

/// The Apple TV's half of pairing (`Pairing.swift`): its code, big enough to read from
/// the sofa, and what to do on the phone. Settings → Account and sync → Sign in with your iPhone.
struct PairWithPhoneView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.auroraTheme) private var theme
    @State private var receiver = PairingReceiver()
    let deviceName: String
    /// Called once the TV is signed in, so Settings reads the new status.
    let signedIn: () -> Void

    var body: some View {
        VStack(spacing: Daylight.px(20)) {
            Image(systemName: "iphone.and.arrow.forward")
                .font(.ui(size: Daylight.px(40), weight: .semibold))
                .foregroundStyle(theme.accent)
            Text("Sign in with your iPhone")
                .displayFont(size: Daylight.px(28))
            Text("On an iPhone or iPad that already syncs, open Aurora, go to Settings → Account and sync, choose Set up an Apple TV, pick \(deviceName), and enter this code.")
                .font(.ui(size: Daylight.px(15)))
                .foregroundStyle(theme.textMuted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: Daylight.px(560))
            status
            Button("Cancel") { dismiss() }
                .buttonStyle(.daylight(.secondary, .md))
        }
        .foregroundStyle(theme.text)
        .padding(Daylight.px(48))
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.background.ignoresSafeArea())
        .onAppear { start() }
        .onDisappear { receiver.stop() }
        .onChange(of: receiver.state) { _, state in
            if case .done = state {
                signedIn()
                Task {
                    try? await Task.sleep(for: .seconds(2))
                    dismiss()
                }
            }
        }
    }

    @ViewBuilder private var status: some View {
        switch receiver.state {
        case .starting:
            ProgressView()
        case .waiting(let code):
            Text(verbatim: spaced(code))
                .font(.ui(size: Daylight.px(56), weight: .heavy).monospacedDigit())
                .tracking(Daylight.px(6))
                .padding(.vertical, Daylight.px(12))
                .padding(.horizontal, Daylight.px(28))
                .background(theme.surfaceSunk, in: RoundedRectangle(cornerRadius: Daylight.px(18), style: .continuous))
                .accessibilityLabel("Code \(code.map(String.init).joined(separator: " "))")
            Text("Waiting for your iPhone…")
                .font(.ui(size: Daylight.px(13), weight: .bold))
                .foregroundStyle(theme.textFaint)
        case .signingIn:
            ProgressView("Signing in…")
        case .done(let message):
            Label(message, systemImage: "checkmark.circle.fill")
                .font(.ui(size: Daylight.px(17), weight: .bold))
                .foregroundStyle(theme.success)
        case .failed(let message):
            Text(message)
                .font(.ui(size: Daylight.px(15)))
                .foregroundStyle(theme.danger)
                .multilineTextAlignment(.center)
        }
    }

    private func spaced(_ code: String) -> String {
        "\(code.prefix(3)) \(code.suffix(3))"
    }

    private func start() {
        guard let core = model.core else { return }
        let name = deviceName
        receiver.start(name: name) { c in
            do {
                let joined = try await Task.detached {
                    try core.syncSignIn(email: c.email, password: c.password, passphrase: c.passphrase,
                                        recoveryCode: nil, deviceName: name)
                }.value
                return Pairing.Reply(ok: true, message: joined.problem == nil
                    ? "\(name) is signed in."
                    : "\(name) is signed in. The first sync will finish when the service can be reached.")
            } catch {
                return Pairing.Reply(ok: false, message: "\(name) could not sign in: \(error.viewerMessage)")
            }
        }
    }
}

/// The phone's half: find the Apple TVs showing a code, and sign one in with this
/// household's account. Settings → Account and sync → Set up an Apple TV.
struct SetUpAppleTVView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.auroraTheme) private var theme
    @State private var sender = PairingSender()
    @State private var chosen: PairingSender.TV?
    @State private var code = ""
    @State private var password = ""
    @State private var passphrase = ""
    @State private var busy = false
    @State private var message: (text: String, ok: Bool)?
    /// The account this device syncs with: the TV joins the same one.
    let email: String

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("On the Apple TV, open Aurora, go to Settings → Account and sync and choose Sign in with your iPhone. It shows a code and appears here.")
                        .font(.ui(.caption)).foregroundStyle(.secondary)
                    if sender.found.isEmpty {
                        HStack(spacing: 10) {
                            ProgressView()
                            Text("Looking for Apple TVs on this network…").foregroundStyle(.secondary)
                        }
                    }
                    ForEach(sender.found) { tv in
                        Button {
                            chosen = tv
                            message = nil
                        } label: {
                            HStack {
                                Label(tv.name, systemImage: "appletv")
                                Spacer()
                                if chosen == tv { Image(systemName: "checkmark").foregroundStyle(theme.accent) }
                            }
                        }
                    }
                } header: {
                    Text("Apple TVs")
                }
                if let chosen {
                    Section {
                        TextField("Code on the TV", text: $code)
                            #if os(iOS)
                            .keyboardType(.numberPad)
                            #endif
                            .textContentType(.oneTimeCode)
                        LabeledContent("Email", value: email)
                        SecureField("Account password", text: $password)
                            .textContentType(.password)
                        SecureField("Sync passphrase", text: $passphrase)
                        Button(busy ? "Signing in…" : "Sign in \(chosen.name)") { pair(chosen) }
                            .disabled(busy || code.filter(\.isNumber).count != 6 || password.isEmpty || passphrase.isEmpty)
                    } header: {
                        Text(chosen.name)
                    } footer: {
                        Text("Sent to the TV over your home network, encrypted for it alone, once the code matches. The TV then signs in exactly as if they were typed on it.")
                    }
                }
                if let message {
                    Text(message.text).foregroundStyle(message.ok ? theme.success : theme.danger)
                }
            }
            .navigationTitle("Set up an Apple TV")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            }
        }
        .onAppear { sender.browse() }
        .onDisappear { sender.stop() }
    }

    private func pair(_ tv: PairingSender.TV) {
        let credentials = Pairing.Credentials(email: email, password: password, passphrase: passphrase)
        busy = true
        message = nil
        Task {
            do {
                let reply = try await sender.pair(tv, code: code, credentials: credentials)
                message = (reply.message, reply.ok)
                if reply.ok {
                    password = ""
                    passphrase = ""
                    code = ""
                    // Signed in: it has stopped listening, and leaves the list.
                    chosen = nil
                }
            } catch {
                message = (error.localizedDescription, false)
            }
            busy = false
        }
    }
}
