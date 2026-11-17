//! Provider passwords, kept in the Apple Keychain.
//!
//! The shared crates already define where a secret goes: `aurora_ingest::credentials::
//! CredentialStore`, implemented on Windows by Credential Manager. On Apple the store is
//! the Keychain, and the Keychain is easiest and safest to reach from Swift
//! (`SecItemAdd` and friends, with the access group and accessibility class the app
//! chooses). So Swift implements [`SecretStore`] and this module adapts it to the
//! trait the shared code already calls. Nothing in `aurora-ingest` changes.
//!
//! The alternative — the `keyring` crate's `apple-native` feature — would mean changing
//! `aurora-ingest`'s manifest, which the Apple port does not do (apple/README.md).

use std::sync::Arc;

use aurora_ingest::credentials::{CredentialError, CredentialStore};

/// Why the Keychain refused. Swift returns one of these from its implementation.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SecretError {
    /// Nothing is stored under that key. Not a failure for `delete`.
    #[error("no secret stored under that key")]
    NotFound,
    /// The Keychain said no, with its own words (an `OSStatus` description).
    #[error("{message}")]
    Backend { message: String },
}

/// A Swift-implemented secret store: the Keychain on Apple devices.
///
/// Keys are opaque strings minted by `aurora_ingest::credentials::credential_ref`; they
/// name no provider and carry no secret, so Swift may use them as the Keychain item's
/// account name as they are.
#[uniffi::export(with_foreign)]
pub trait SecretStore: Send + Sync {
    fn set(&self, key: String, secret: String) -> Result<(), SecretError>;
    fn get(&self, key: String) -> Result<String, SecretError>;
    fn delete(&self, key: String) -> Result<(), SecretError>;
}

/// The Swift store, seen through the trait the shared crates call.
pub(crate) struct Bridged(pub Arc<dyn SecretStore>);

fn into_credential_error(key: &str, e: SecretError) -> CredentialError {
    match e {
        SecretError::NotFound => CredentialError::NotFound(key.to_string()),
        SecretError::Backend { message } => CredentialError::Backend(message),
    }
}

impl CredentialStore for Bridged {
    fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        self.0
            .set(key.to_string(), secret.to_string())
            .map_err(|e| into_credential_error(key, e))
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.0
            .get(key.to_string())
            .map_err(|e| into_credential_error(key, e))
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        match self.0.delete(key.to_string()) {
            // Deleting what is not there leaves the store as asked.
            Ok(()) | Err(SecretError::NotFound) => Ok(()),
            Err(e) => Err(into_credential_error(key, e)),
        }
    }

    /// The Keychain survives a restart. A Swift implementation that does not (a test
    /// double) is still reported as persistent, because nothing on Apple shows the
    /// warning this flag drives on Windows.
    fn is_persistent(&self) -> bool {
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// What Swift's Keychain store does, in memory, for the tests.
    #[derive(Default)]
    pub(crate) struct MemorySecrets(Mutex<HashMap<String, String>>);

    impl SecretStore for MemorySecrets {
        fn set(&self, key: String, secret: String) -> Result<(), SecretError> {
            self.0.lock().unwrap().insert(key, secret);
            Ok(())
        }
        fn get(&self, key: String) -> Result<String, SecretError> {
            self.0
                .lock()
                .unwrap()
                .get(&key)
                .cloned()
                .ok_or(SecretError::NotFound)
        }
        fn delete(&self, key: String) -> Result<(), SecretError> {
            self.0
                .lock()
                .unwrap()
                .remove(&key)
                .map(|_| ())
                .ok_or(SecretError::NotFound)
        }
    }

    #[test]
    fn a_secret_comes_back_through_the_shared_trait() {
        let store = Bridged(Arc::new(MemorySecrets::default()));
        store.set("k", "hunter2").unwrap();
        assert_eq!(store.get("k").unwrap(), "hunter2");
    }

    #[test]
    fn a_missing_secret_is_not_found_not_a_backend_failure() {
        let store = Bridged(Arc::new(MemorySecrets::default()));
        assert!(matches!(store.get("k"), Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn deleting_what_is_not_there_is_fine() {
        let store = Bridged(Arc::new(MemorySecrets::default()));
        assert!(store.delete("k").is_ok());
    }
}
