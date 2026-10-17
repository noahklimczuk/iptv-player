//! Turning a stored stream reference back into a playable address.
//!
//! An Xtream stream URL carries the credentials in its path — `{base}/live/{user}/{pass}/
//! {id}.ts`, and `/movie/` and `/series/` the same — and those strings used to be stored
//! verbatim in `channel_sources.url`, `movies.url` and `episodes.url`. On the
//! subscription this project is measured against that is 6,699 channel sources, 27,494
//! films and one row per episode of 8,086 series: several hundred thousand plaintext
//! copies of the username and password in a single SQLite file, while
//! `docs/PRIVACY.md` promised that a copy of `library.db` "contains no credentials" —
//! the specific reassurance somebody reads before attaching it to a bug report (R-126).
//!
//! So the rows keep `stream_id` and `container`, and the address is rebuilt here. This
//! module is in `aurora-app` and not in `aurora-db` for one reason: the password lives in
//! Credential Manager, and the database layer has no business being able to reach it.
//!
//! **What this changes for a viewer.** A library whose credential entry has gone — the
//! password cleared in Settings, a profile copied from another machine — is no longer
//! playable from the stored URLs, because there are no stored URLs. The refusal says to
//! enter the password again rather than failing at mpv with a 401.

use std::collections::HashMap;

use aurora_core::model::MediaKind;
use aurora_db::rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};

/// A provider's address and credentials, read once per playback session.
#[derive(Debug, Clone)]
pub struct ProviderAccess {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

/// What a row holds instead of a URL.
#[derive(Debug, Clone)]
pub struct StreamRef {
    /// The stored address. Non-empty for M3U rows, which cannot be rebuilt.
    pub url: String,
    pub stream_id: Option<String>,
    pub container: Option<String>,
    /// Which provider's credentials rebuild it.
    pub provider_id: Option<i64>,
}

impl StreamRef {
    /// Whether this row needs the provider's credentials to become an address.
    pub fn needs_rebuilding(&self) -> bool {
        self.url.trim().is_empty() && self.stream_id.is_some()
    }
}

impl From<&aurora_db::repo::sources::Source> for StreamRef {
    fn from(source: &aurora_db::repo::sources::Source) -> Self {
        Self {
            url: source.url.clone(),
            stream_id: source.stream_id.clone(),
            container: source.container.clone(),
            provider_id: Some(source.provider_id),
        }
    }
}

/// The address to hand to the player.
///
/// The stored URL when there is one — an M3U provider's URLs are whatever the playlist
/// said — and otherwise `xtream::stream_url` over the provider's base address and the
/// password from the credential store.
pub fn playable(
    db: &Connection,
    credentials: &dyn aurora_ingest::credentials::CredentialStore,
    cache: &mut HashMap<i64, ProviderAccess>,
    kind: MediaKind,
    stream: &StreamRef,
) -> Result<String> {
    if !stream.url.trim().is_empty() {
        return Ok(stream.url.clone());
    }
    let Some(stream_id) = stream.stream_id.as_deref().filter(|s| !s.is_empty()) else {
        // Neither an address nor a reference. Handed back as it is, because the callers
        // already have better words for it than this function does: `window::playable`
        // says "this film is in your library without a playable address" and names the
        // kind, which is the message that finding asked for.
        return Ok(stream.url.clone());
    };
    let provider_id = stream.provider_id.ok_or_else(|| {
        AppError::Other(
            "This title is not attached to a provider any more, so its stream address \
             cannot be rebuilt. A refresh should restore it."
                .into(),
        )
    })?;

    let access = match cache.get(&provider_id) {
        Some(access) => access.clone(),
        None => {
            let access = provider_access(db, credentials, provider_id)?;
            cache.insert(provider_id, access.clone());
            access
        }
    };

    let id: u32 = stream_id.parse().map_err(|_| {
        AppError::Other(format!(
            "This title's stream id ({stream_id}) is not a number, so its address \
             cannot be rebuilt. A refresh should restore it."
        ))
    })?;
    Ok(aurora_core::xtream::stream_url(
        &access.base_url,
        &access.username,
        &access.password,
        kind,
        id,
        stream.container.as_deref(),
    ))
}

/// A provider's base address, username and password.
///
/// The password comes from the credential store and nowhere else. A provider that has a
/// `credential_ref` the store will not give back is a different thing from one with no
/// reference at all, and the message says which — the same distinction `stored_password`
/// draws in `providers.rs`, for the same reason.
fn provider_access(
    db: &Connection,
    credentials: &dyn aurora_ingest::credentials::CredentialStore,
    provider_id: i64,
) -> Result<ProviderAccess> {
    let row: Option<(String, String, Option<String>)> = db
        .query_row(
            "SELECT base_url, COALESCE(username, ''), credential_ref
             FROM providers WHERE id = ?1",
            params![provider_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(aurora_db::DbError::from)?;

    let (base_url, username, credential_ref_value) = row.ok_or_else(|| {
        AppError::Other(
            "The provider this title came from is no longer set up, so its stream \
             address cannot be rebuilt."
                .into(),
        )
    })?;

    let key = credential_ref_value.ok_or_else(|| {
        AppError::Other(
            "This provider has no saved password, so Aurora cannot build the stream \
             address. Edit the provider in Settings and enter it again."
                .into(),
        )
    })?;
    let password = credentials.get(&key).map_err(|e| {
        AppError::Other(format!(
            "Aurora could not read this provider's saved password: {e}. Edit the \
             provider in Settings and enter it again."
        ))
    })?;

    Ok(ProviderAccess {
        base_url,
        username,
        password,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_ingest::credentials::{CredentialStore, MemoryStore};

    fn seeded() -> Connection {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,username,credential_ref,created_at)
                 VALUES (1,'Panel','xtream','http://panel.example:8080/','bob','ref/1',0);
             INSERT INTO providers (id,name,kind,base_url,created_at)
                 VALUES (2,'List','m3u','http://host/get.php',0);
             INSERT INTO providers (id,name,kind,base_url,username,created_at)
                 VALUES (3,'Lapsed','xtream','http://panel.example:8080','bob',0);",
        )
        .unwrap();
        conn
    }

    fn store() -> MemoryStore {
        let store = MemoryStore::default();
        store.set("ref/1", "hunter2").unwrap();
        store
    }

    fn xtream(stream_id: &str, container: Option<&str>) -> StreamRef {
        StreamRef {
            url: String::new(),
            stream_id: Some(stream_id.to_string()),
            container: container.map(str::to_string),
            provider_id: Some(1),
        }
    }

    /// The address the player gets is the one the panel expects.
    #[test]
    fn a_stream_id_becomes_the_address_the_panel_serves() {
        let db = seeded();
        let mut cache = HashMap::new();
        let url = playable(
            &db,
            &store(),
            &mut cache,
            MediaKind::Live,
            &xtream("1234", Some("ts")),
        )
        .unwrap();
        assert_eq!(url, "http://panel.example:8080/live/bob/hunter2/1234.ts");

        let movie = playable(
            &db,
            &store(),
            &mut cache,
            MediaKind::Movie,
            &xtream("555", Some("mkv")),
        )
        .unwrap();
        assert_eq!(movie, "http://panel.example:8080/movie/bob/hunter2/555.mkv");

        // No container stored — an older row, or a panel that did not say — falls back
        // to the extension `stream_url` has always defaulted to.
        let bare = playable(
            &db,
            &store(),
            &mut cache,
            MediaKind::Movie,
            &xtream("555", None),
        )
        .unwrap();
        assert_eq!(bare, "http://panel.example:8080/movie/bob/hunter2/555.mp4");
    }

    /// The credential store is asked once, however many rows are resolved.
    #[test]
    fn one_provider_is_one_credential_read() {
        /// Counts `get` calls, because a mosaic resolves nine addresses at once and a
        /// keyring read is a syscall rather than a lookup.
        struct Counting {
            inner: MemoryStore,
            reads: std::sync::atomic::AtomicUsize,
        }
        impl CredentialStore for Counting {
            fn set(
                &self,
                k: &str,
                v: &str,
            ) -> std::result::Result<(), aurora_ingest::credentials::CredentialError> {
                self.inner.set(k, v)
            }
            fn get(
                &self,
                k: &str,
            ) -> std::result::Result<String, aurora_ingest::credentials::CredentialError>
            {
                self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.inner.get(k)
            }
            fn delete(
                &self,
                k: &str,
            ) -> std::result::Result<(), aurora_ingest::credentials::CredentialError> {
                self.inner.delete(k)
            }
            fn is_persistent(&self) -> bool {
                false
            }
        }

        let db = seeded();
        let counting = Counting {
            inner: store(),
            reads: std::sync::atomic::AtomicUsize::new(0),
        };
        let mut cache = HashMap::new();
        for id in ["1", "2", "3", "4"] {
            playable(
                &db,
                &counting,
                &mut cache,
                MediaKind::Live,
                &xtream(id, Some("ts")),
            )
            .unwrap();
        }
        assert_eq!(counting.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A stored URL is handed back untouched, which is what an M3U row holds.
    #[test]
    fn a_stored_url_is_used_as_it_is() {
        let db = seeded();
        let mut cache = HashMap::new();
        let stream = StreamRef {
            url: "http://host/stream/9.mkv".into(),
            stream_id: None,
            container: None,
            provider_id: Some(2),
        };
        let url = playable(&db, &store(), &mut cache, MediaKind::Movie, &stream).unwrap();
        assert_eq!(url, "http://host/stream/9.mkv");
    }

    /// A provider whose password has gone says so, rather than failing at mpv.
    ///
    /// This is the behaviour change the fix brings: there is no stored URL to fall back
    /// on any more, so a library whose credential entry is missing is not playable until
    /// the password is entered again. The message has to say that (R-126).
    #[test]
    fn a_missing_password_says_what_to_do_about_it() {
        let db = seeded();
        let mut cache = HashMap::new();

        // Provider 3 has no `credential_ref` at all.
        let mut stream = xtream("7", Some("ts"));
        stream.provider_id = Some(3);
        let err = playable(&db, &store(), &mut cache, MediaKind::Live, &stream)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Settings"), "{err}");
        assert!(err.contains("password"), "{err}");

        // And provider 1's reference pointing at nothing is a different sentence, for
        // the same reason `stored_password` draws that line in `providers.rs`.
        let empty = MemoryStore::default();
        let err = playable(
            &db,
            &empty,
            &mut cache,
            MediaKind::Live,
            &xtream("7", Some("ts")),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("could not read"), "{err}");
    }

    /// A row with no address and no reference is left to the caller's own words.
    #[test]
    fn nothing_to_go_on_is_handed_back_as_it_is() {
        let db = seeded();
        let mut cache = HashMap::new();
        let stream = StreamRef {
            url: "   ".into(),
            stream_id: None,
            container: None,
            provider_id: Some(1),
        };
        // `window::playable` is the one that says "in your library without a playable
        // address" and names the kind, so this must not pre-empt it.
        let url = playable(&db, &store(), &mut cache, MediaKind::Movie, &stream).unwrap();
        assert_eq!(url, "   ");
    }
}
