//! Credential storage (README C10: never plaintext, never in the database, never in
//! a log or an export).
//!
//! Same seam as the playback backend (docs/DECISIONS.md D3): a trait, a real
//! platform-backed implementation gated to Windows, and an in-memory one that
//! compiles everywhere so the rest of the pipeline is testable on any host.

use std::collections::HashMap;
use std::sync::Mutex;

/// Where the per-library namespace is kept once it has been made.
const NAMESPACE_KEY: &str = "credentials.namespace";

/// The opaque handle persisted in `providers.credential_ref`. Knowing it reveals
/// nothing; it is only a key into the OS store.
///
/// **The namespace is the whole point.** This used to be `aurora-provider-{id}`, and a
/// provider id is a SQLite row id that starts at 1 in every new library. The credential
/// store is not per-library, though, and on Windows it is not even per-application: it
/// is one Credential Manager for the whole user account. So two copies of Aurora on one
/// account — an installed one and a portable one, say — both call their first provider
/// `aurora-provider-1` and write to the same entry. Deleting the provider in one, or
/// failing to save one, took the *other* copy's password with it, leaving a provider
/// row that lists fine and cannot authenticate: it disappears with no error that says
/// why, because as far as the app is concerned it simply has no password.
///
/// It is worth being plain that this is not a leak. The namespace is random and means
/// nothing; it exists so that two libraries name different entries.
pub fn credential_ref(namespace: &str, provider_id: i64) -> String {
    format!("aurora-{namespace}-provider-{provider_id}")
}

/// This library's namespace, made once and then kept.
///
/// Random rather than derived from the data directory, because a portable copy that is
/// moved is still the same library and must keep its passwords. Stored in `settings`,
/// so it travels with the library it belongs to.
///
/// Providers saved before this existed keep working untouched: their key is recorded in
/// `providers.credential_ref` and every read and delete goes through that column, so an
/// old `aurora-provider-1` is still found under exactly that name.
pub fn namespace(conn: &aurora_db::rusqlite::Connection) -> Result<String, CredentialError> {
    use aurora_db::repo::settings;

    if let Some(existing) = settings::get::<String>(conn, NAMESPACE_KEY)
        .map_err(|e| CredentialError::Backend(e.to_string()))?
    {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }

    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| CredentialError::Backend(e.to_string()))?;
    let made: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    settings::set(conn, NAMESPACE_KEY, &made)
        .map_err(|e| CredentialError::Backend(e.to_string()))?;
    Ok(made)
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("no credential stored for {0}")]
    NotFound(String),
    #[error("the credential store refused the request: {0}")]
    Backend(String),
}

pub trait CredentialStore: Send + Sync {
    fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError>;
    fn get(&self, key: &str) -> Result<String, CredentialError>;
    fn delete(&self, key: &str) -> Result<(), CredentialError>;
    /// Whether secrets survive a restart. False for the in-memory fallback, so the UI
    /// can say so rather than silently losing a provider's password.
    fn is_persistent(&self) -> bool;
}

/// Process-lifetime store. Used by tests, and on non-Windows hosts where there is no
/// Credential Manager — it reports `is_persistent() == false` so nothing pretends
/// otherwise.
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, String>>,
}

impl CredentialStore for MemoryStore {
    fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        self.entries
            .lock()
            .map_err(|e| CredentialError::Backend(e.to_string()))?
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.entries
            .lock()
            .map_err(|e| CredentialError::Backend(e.to_string()))?
            .get(key)
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(key.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.entries
            .lock()
            .map_err(|e| CredentialError::Backend(e.to_string()))?
            .remove(key);
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

#[cfg(windows)]
mod windows_store {
    use super::{CredentialError, CredentialStore};

    /// Backed by Windows Credential Manager.
    #[derive(Debug, Default)]
    pub struct KeyringStore;

    const SERVICE: &str = "AuroraTV";

    /// The environment variable that moves this store somewhere else.
    ///
    /// Credential Manager is per *user*, not per data directory -- so a service name
    /// fixed at `AuroraTV` meant every run of `tests-host` read, and could overwrite,
    /// the real app's saved provider passwords and API keys. The data directory being
    /// redirected (`AURORA_DATA_DIR`, or the portable marker) isolated everything except
    /// this, which was the one store that ignored it.
    ///
    /// It was not hypothetical: a scenario asking for `gemini.status` against a brand new
    /// portable profile came back `hasKey: true`, which it could only have got from the
    /// real vault.
    pub const SERVICE_ENV: &str = "AURORA_CREDENTIAL_SERVICE";

    /// Where secrets are filed. Overridable, so a test run can be kept apart.
    ///
    /// Read per call rather than cached: cheap next to the vault round trip that follows
    /// it, and a cached value would be read before a test could set one.
    fn service() -> String {
        service_from(std::env::var(SERVICE_ENV).ok())
    }

    /// The decision, separated from reading the environment so it can be tested without
    /// one -- an env-var test races every other test in the binary.
    fn service_from(override_name: Option<String>) -> String {
        match override_name {
            Some(name) if !name.trim().is_empty() => name,
            _ => SERVICE.to_string(),
        }
    }

    fn entry(key: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(&service(), key).map_err(|e| CredentialError::Backend(e.to_string()))
    }

    impl CredentialStore for KeyringStore {
        fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
            entry(key)?
                .set_password(secret)
                .map_err(|e| CredentialError::Backend(e.to_string()))
        }

        fn get(&self, key: &str) -> Result<String, CredentialError> {
            match entry(key)?.get_password() {
                Ok(v) => Ok(v),
                Err(keyring::Error::NoEntry) => Err(CredentialError::NotFound(key.to_string())),
                Err(e) => Err(CredentialError::Backend(e.to_string())),
            }
        }

        fn delete(&self, key: &str) -> Result<(), CredentialError> {
            match entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(CredentialError::Backend(e.to_string())),
            }
        }

        /// What the store actually is, not what it is meant to be.
        ///
        /// This returned a hard-coded `true` while the crate was quietly falling back
        /// to its mock, so the app both lost every password and said it had kept them.
        /// keyring knows the answer; asking it is the only version of this that cannot
        /// drift from reality.
        fn is_persistent(&self) -> bool {
            matches!(
                keyring::default::default_credential_builder().persistence(),
                keyring::credential::CredentialPersistence::UntilDelete
            )
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The default, which is where the real app's secrets live.
        #[test]
        fn with_no_override_it_is_the_real_vault() {
            assert_eq!(service_from(None), SERVICE);
        }

        /// And the override, which is what keeps a test run out of it. Credential Manager
        /// is per user rather than per data directory, so this is the only thing that
        /// separates a harness run's secrets from the installed copy's.
        #[test]
        fn an_override_moves_the_store_somewhere_else() {
            assert_eq!(
                service_from(Some("AuroraTV-tests-host".into())),
                "AuroraTV-tests-host"
            );
            assert_ne!(service_from(Some("AuroraTV-tests-host".into())), SERVICE);
        }

        /// An unset variable arrives as an empty string often enough -- an exported but
        /// empty shell variable, a CI expression over a secret that is not set -- and
        /// filing secrets under `""` is not a sensible reading of that.
        #[test]
        fn an_empty_or_blank_override_is_not_a_name() {
            for blank in ["", " ", "	"] {
                assert_eq!(service_from(Some(blank.into())), SERVICE, "{blank:?}");
            }
        }

        fn scratch_key() -> String {
            format!(
                "aurora-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            )
        }

        /// The test that would have caught it.
        ///
        /// Separate calls, because that is the whole bug: `providers_save` writes
        /// through one `Entry` and `providers_refresh` reads through another, and
        /// keyring's mock gives each `Entry` its own empty credential. A round trip
        /// inside a single `Entry` would have passed against the mock and proved
        /// nothing.
        #[test]
        fn a_password_survives_being_written_and_read_back_by_separate_calls() {
            let store = KeyringStore;
            let key = scratch_key();

            store
                .set(&key, "hunter2")
                .expect("write to the credential store");
            let got = store.get(&key).expect("read it back");
            assert_eq!(got, "hunter2");

            store.delete(&key).expect("delete it again");
            assert!(
                matches!(store.get(&key), Err(CredentialError::NotFound(_))),
                "a deleted credential must be gone, not stale"
            );
        }

        #[test]
        fn the_store_is_the_real_credential_manager_and_says_so() {
            assert!(
                KeyringStore.is_persistent(),
                "keyring fell back to its mock, which keeps secrets in the Entry and \
                 loses them the moment it drops — check the windows-native feature"
            );
        }
    }
}

#[cfg(windows)]
pub use windows_store::KeyringStore;

/// The best store this platform offers.
pub fn default_store() -> Box<dyn CredentialStore> {
    #[cfg(windows)]
    {
        Box::new(KeyringStore)
    }
    #[cfg(not(windows))]
    {
        tracing::warn!(
            "no OS credential store on this platform; provider passwords will be kept \
             in memory only and lost on exit"
        );
        Box::<MemoryStore>::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_refs_are_stable_and_reveal_nothing() {
        assert_eq!(credential_ref("ab12", 7), credential_ref("ab12", 7));
        assert_ne!(credential_ref("ab12", 7), credential_ref("ab12", 8));
        assert!(!credential_ref("ab12", 7).contains("password"));
    }

    #[test]
    fn two_libraries_never_name_the_same_entry() {
        // The bug this exists for. Provider ids start at 1 in every library, and the
        // credential store is one per user account — so before namespacing, a second
        // copy of Aurora wrote its first provider's password over the first copy's, and
        // deleting a provider in one silently took the other's password with it.
        assert_ne!(credential_ref("ab12", 1), credential_ref("cd34", 1));
    }

    #[test]
    fn a_namespace_is_made_once_and_then_kept() {
        let conn = aurora_db::open_memory().unwrap();
        let first = namespace(&conn).unwrap();
        assert_eq!(first.len(), 16, "eight random bytes as hex");
        assert_eq!(
            namespace(&conn).unwrap(),
            first,
            "it must not move under a library"
        );

        // And a different library gets a different one, which is the whole point.
        let other = aurora_db::open_memory().unwrap();
        assert_ne!(namespace(&other).unwrap(), first);
    }

    #[test]
    fn round_trips_a_secret() {
        let store = MemoryStore::default();
        store.set("k", "hunter2").unwrap();
        assert_eq!(store.get("k").unwrap(), "hunter2");
    }

    #[test]
    fn overwrites_rather_than_duplicating() {
        let store = MemoryStore::default();
        store.set("k", "old").unwrap();
        store.set("k", "new").unwrap();
        assert_eq!(store.get("k").unwrap(), "new");
    }

    #[test]
    fn a_missing_key_is_not_found_rather_than_empty() {
        let store = MemoryStore::default();
        assert!(matches!(
            store.get("nope"),
            Err(CredentialError::NotFound(_))
        ));
    }

    #[test]
    fn delete_removes_and_is_idempotent() {
        let store = MemoryStore::default();
        store.set("k", "v").unwrap();
        store.delete("k").unwrap();
        assert!(store.get("k").is_err());
        store.delete("k").unwrap();
    }

    #[test]
    fn the_memory_store_admits_it_does_not_persist() {
        assert!(!MemoryStore::default().is_persistent());
    }

    #[test]
    fn a_secret_never_appears_in_the_error_text() {
        let store = MemoryStore::default();
        let err = store.get("aurora-provider-1").unwrap_err().to_string();
        assert!(!err.contains("hunter2"));
        assert!(
            err.contains("aurora-provider-1"),
            "the key is fine to name: {err}"
        );
    }
}
