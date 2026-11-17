import Foundation
import Security

/// Provider passwords in the Keychain: the Apple counterpart of Windows Credential
/// Manager (README C10).
///
/// The Rust core calls this through the `SecretStore` protocol UniFFI generates from
/// `apple/core/aurora-ffi/src/secrets.rs`. Keys are opaque references minted by
/// `aurora_ingest::credentials::credential_ref`; they name no provider and hold no secret.
final class KeychainSecretStore: SecretStore, @unchecked Sendable {
    /// One service name for every Aurora secret, so they are easy to find and remove.
    private let service: String

    init(service: String = "tv.aurora.provider-password") {
        self.service = service
    }

    func set(key: String, secret: String) throws {
        let data = Data(secret.utf8)
        let match = baseQuery(key)
        let update: [String: Any] = [kSecValueData as String: data]

        var status = SecItemUpdate(match as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var add = match
            add[kSecValueData as String] = data
            // Readable after the first unlock, so a background refresh can sign in, and
            // never migrated to another device by a backup: a new device signs in itself.
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            status = SecItemAdd(add as CFDictionary, nil)
        }
        try check(status)
    }

    func get(key: String) throws -> String {
        var query = baseQuery(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        try check(status)
        guard let data = item as? Data, let secret = String(data: data, encoding: .utf8) else {
            throw SecretError.Backend(message: "The saved password could not be read.")
        }
        return secret
    }

    func delete(key: String) throws {
        try check(SecItemDelete(baseQuery(key) as CFDictionary))
    }

    private func baseQuery(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
    }

    private func check(_ status: OSStatus) throws {
        switch status {
        case errSecSuccess:
            return
        case errSecItemNotFound:
            throw SecretError.NotFound
        default:
            let reason = SecCopyErrorMessageString(status, nil) as String? ?? "OSStatus \(status)"
            throw SecretError.Backend(message: "The Keychain refused: \(reason)")
        }
    }
}
