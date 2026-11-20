import CryptoKit
import Foundation
import Network

/// Signing an Apple TV in to the household's sync from a phone, so nothing is typed with
/// a remote (docs/APPLE_PORT_AND_SYNC.md §2.4 and §3.6, Phase 5).
///
/// The plan pairs through a `pairing/{id}` document on the server. That needs a rule the
/// shared Firestore rules don't have, and the port keeps to `apple/`, so the two devices
/// talk over the home network instead, which is where a TV and the phone setting it up
/// are anyway:
///
/// 1. The TV listens, advertised over Bonjour as `_aurora-pair._tcp`, with a fresh X25519
///    key, and shows a six-digit code made from that key.
/// 2. The phone finds it, connects, and is sent the TV's key. The viewer types the code
///    the TV shows; a key whose code differs is not the TV on the screen, and the phone
///    stops there.
/// 3. The phone sends its own key and, sealed with ChaCha20-Poly1305 under the shared
///    secret, the account's email and password and the sync passphrase.
/// 4. The TV signs in with them exactly as its own form would (`sync_sign_in`) and says
///    how that went.
///
/// The TV listens only while its pairing screen is up, and forgets its key when it closes.
enum Pairing {
    static let service = "_aurora-pair._tcp"
    static let version = 1

    /// What the phone hands over: what the TV's own sign-in form asks for.
    struct Credentials: Codable, Sendable {
        let email: String
        let password: String
        let passphrase: String
    }

    /// The TV's first message.
    struct Hello: Codable, Sendable {
        let version: Int
        let key: Data
    }

    /// The phone's answer.
    struct Offer: Codable, Sendable {
        let key: Data
        /// `Credentials` as JSON, sealed (`ChaChaPoly.SealedBox.combined`).
        let sealed: Data
    }

    /// The TV's last word: whether it is signed in, in the words to show on the phone.
    struct Reply: Codable, Sendable {
        let ok: Bool
        let message: String
    }

    enum Failure: LocalizedError {
        case wrongCode
        case closed
        case unreadable
        case newerVersion

        var errorDescription: String? {
            switch self {
            case .wrongCode: "That code isn't the one on this Apple TV. Check the code on the TV screen and try again."
            case .closed: "The Apple TV stopped listening. Open Settings → Sync on the TV and choose Sign in with your iPhone again."
            case .unreadable: "The Apple TV sent something this version of Aurora can't read."
            case .newerVersion: "The Apple TV has a newer version of Aurora. Update Aurora on this device first."
            }
        }
    }

    /// The six digits a key is shown as: the first four bytes of its SHA-256, as a number.
    /// A stranger on the network would need a key that happens to hash to the code on
    /// the screen, a one-in-a-million guess they get one try at.
    static func code(for key: Data) -> String {
        let digest = Array(SHA256.hash(data: key))
        let n = digest.prefix(4).reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
        return String(format: "%06u", n % 1_000_000)
    }

    /// Both ends' keys go into the derivation, so a box made for one pairing opens in no other.
    static func sealingKey(_ secret: SharedSecret, tv: Data, phone: Data) -> SymmetricKey {
        secret.hkdfDerivedSymmetricKey(using: SHA256.self, salt: Data("aurora-pair-v1".utf8),
                                       sharedInfo: tv + phone, outputByteCount: 32)
    }

    // MARK: Framing: each message is a JSON object behind its length as four bytes.

    static func send<T: Encodable>(_ value: T, on connection: NWConnection) async throws {
        let body = try JSONEncoder().encode(value)
        var length = UInt32(body.count).bigEndian
        let frame = Data(bytes: &length, count: 4) + body
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            connection.send(content: frame, completion: .contentProcessed { error in
                if let error { done.resume(throwing: error) } else { done.resume() }
            })
        }
    }

    static func receive<T: Decodable>(_ type: T.Type, on connection: NWConnection) async throws -> T {
        let head = try await read(4, on: connection)
        let length = head.reduce(0) { $0 << 8 | Int($1) }
        // Credentials and keys are a few hundred bytes; anything far past that is not ours.
        guard length > 0, length < 64 * 1024 else { throw Failure.unreadable }
        let body = try await read(length, on: connection)
        guard let value = try? JSONDecoder().decode(T.self, from: body) else { throw Failure.unreadable }
        return value
    }

    private static func read(_ count: Int, on connection: NWConnection) async throws -> Data {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: count, maximumLength: count) { data, _, complete, error in
                if let error {
                    done.resume(throwing: error)
                } else if let data, data.count == count {
                    done.resume(returning: data)
                } else {
                    done.resume(throwing: complete ? Failure.closed : Failure.unreadable)
                }
            }
        }
    }

    /// Wait for a connection to be ready, or say why it never will be.
    static func ready(_ connection: NWConnection, queue: DispatchQueue) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            let once = Once()
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready: once.run { done.resume() }
                case .failed(let error), .waiting(let error):
                    once.run { done.resume(throwing: error) }
                    connection.cancel()
                case .cancelled: once.run { done.resume(throwing: Failure.closed) }
                default: break
                }
            }
            connection.start(queue: queue)
        }
    }
}

/// Runs its body the first time only: a connection reports several states, and a
/// continuation may be resumed once.
private final class Once: @unchecked Sendable {
    private var done = false
    private let lock = NSLock()
    func run(_ body: () -> Void) {
        lock.lock()
        defer { lock.unlock() }
        guard !done else { return }
        done = true
        body()
    }
}

// MARK: - The TV's end

/// Listens for a phone while the TV's pairing screen is up.
@MainActor
@Observable
final class PairingReceiver {
    enum State: Equatable {
        case starting
        /// Advertised; the code to show.
        case waiting(code: String)
        case signingIn
        case done(String)
        case failed(String)
    }

    private(set) var state: State = .starting
    private var listener: NWListener?
    private var key = Curve25519.KeyAgreement.PrivateKey()
    private let queue = DispatchQueue(label: "tv.aurora.pairing")
    /// A phone is connected now.
    private var talking = false

    var code: String { Pairing.code(for: key.publicKey.rawRepresentation) }

    /// `signIn` does what the TV's own form does with what was typed, and returns what
    /// to tell the phone.
    func start(name: String, signIn: @escaping @Sendable (Pairing.Credentials) async -> Pairing.Reply) {
        stop()
        key = Curve25519.KeyAgreement.PrivateKey()
        do {
            let listener = try NWListener(using: .tcp)
            listener.service = NWListener.Service(name: name, type: Pairing.service)
            listener.stateUpdateHandler = { [weak self] state in
                Task { @MainActor in self?.listenerChanged(state) }
            }
            listener.newConnectionHandler = { [weak self] connection in
                Task { @MainActor in self?.accept(connection, signIn: signIn) }
            }
            listener.start(queue: queue)
            self.listener = listener
        } catch {
            state = .failed("Couldn't listen on this network: \(error.localizedDescription)")
        }
    }

    func stop() {
        listener?.cancel()
        listener = nil
    }

    private func listenerChanged(_ change: NWListener.State) {
        switch change {
        case .ready: if state == .starting { state = .waiting(code: code) }
        case .failed(let error): state = .failed("Couldn't listen on this network: \(error.localizedDescription)")
        default: break
        }
    }

    private func accept(_ connection: NWConnection, signIn: @escaping @Sendable (Pairing.Credentials) async -> Pairing.Reply) {
        // One phone at a time, and none once signed in.
        guard case .waiting = state, !talking else {
            connection.cancel()
            return
        }
        talking = true
        let key = self.key
        let queue = self.queue
        Task {
            defer { talking = false }
            do {
                try await Pairing.ready(connection, queue: queue)
                let mine = key.publicKey.rawRepresentation
                try await Pairing.send(Pairing.Hello(version: Pairing.version, key: mine), on: connection)
                let offer = try await Pairing.receive(Pairing.Offer.self, on: connection)
                let theirs = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: offer.key)
                let secret = try key.sharedSecretFromKeyAgreement(with: theirs)
                let box = try ChaChaPoly.SealedBox(combined: offer.sealed)
                let plain = try ChaChaPoly.open(box, using: Pairing.sealingKey(secret, tv: mine, phone: offer.key))
                let credentials = try JSONDecoder().decode(Pairing.Credentials.self, from: plain)
                state = .signingIn
                let reply = await signIn(credentials)
                try? await Pairing.send(reply, on: connection)
                connection.cancel()
                if reply.ok {
                    state = .done(reply.message)
                    stop()
                } else {
                    // Wrong password, say: the phone can try again with the same code.
                    state = .waiting(code: code)
                }
            } catch {
                connection.cancel()
                if state == .signingIn { state = .waiting(code: code) }
            }
        }
    }
}

// MARK: - The phone's end

/// Finds Apple TVs waiting to be paired, and signs one in.
@MainActor
@Observable
final class PairingSender {
    struct TV: Identifiable, Hashable {
        let name: String
        let endpoint: NWEndpoint
        var id: String { name }
    }

    private(set) var found: [TV] = []
    private(set) var browsing = false
    private var browser: NWBrowser?
    private let queue = DispatchQueue(label: "tv.aurora.pairing.phone")

    func browse() {
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjour(type: Pairing.service, domain: nil), using: .tcp)
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            let tvs = results.compactMap { result -> TV? in
                guard case .service(let name, _, _, _) = result.endpoint else { return nil }
                return TV(name: name, endpoint: result.endpoint)
            }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
            Task { @MainActor in self?.found = tvs }
        }
        browser.start(queue: queue)
        self.browser = browser
        browsing = true
    }

    func stop() {
        browser?.cancel()
        browser = nil
        browsing = false
    }

    /// Hand the credentials to `tv`, once its key matches the code typed from its screen.
    func pair(_ tv: TV, code: String, credentials: Pairing.Credentials) async throws -> Pairing.Reply {
        let connection = NWConnection(to: tv.endpoint, using: .tcp)
        defer { connection.cancel() }
        try await Pairing.ready(connection, queue: queue)
        let hello = try await Pairing.receive(Pairing.Hello.self, on: connection)
        guard hello.version <= Pairing.version else { throw Pairing.Failure.newerVersion }
        let typed = code.filter(\.isNumber)
        guard typed == Pairing.code(for: hello.key) else { throw Pairing.Failure.wrongCode }
        let mine = Curve25519.KeyAgreement.PrivateKey()
        let theirs = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: hello.key)
        let secret = try mine.sharedSecretFromKeyAgreement(with: theirs)
        let phoneKey = mine.publicKey.rawRepresentation
        let sealed = try ChaChaPoly.seal(JSONEncoder().encode(credentials),
                                         using: Pairing.sealingKey(secret, tv: hello.key, phone: phoneKey))
        try await Pairing.send(Pairing.Offer(key: phoneKey, sealed: sealed.combined), on: connection)
        return try await Pairing.receive(Pairing.Reply.self, on: connection)
    }
}
