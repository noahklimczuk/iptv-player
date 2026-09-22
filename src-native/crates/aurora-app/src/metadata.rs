//! Metadata enrichment commands (README §4.5).
//!
//! The key lives in the credential store, so it is never in the database, never in a
//! backup, and never in an export. Commands here deal in "is a key set", never in the
//! key itself — nothing ever sends it back to the UI.

use std::sync::Arc;

use aurora_db::repo::enrichment::{self, Coverage, CreditRow, ItemKind};
use aurora_ingest::enrich::{self, Options, Report};
use aurora_ingest::tmdb::{TmdbClient, CREDENTIAL_KEY};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::error::Result;
use crate::services::Services;
use crate::AppError;

/// What the metadata panel in settings shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStatus {
    /// Whether a key is stored. Never the key.
    pub has_key: bool,
    /// False when the key would be lost on restart, so the UI can say so.
    pub key_is_persistent: bool,
    pub movies: Coverage,
    pub series: Coverage,
}

#[tauri::command]
pub fn metadata_status(services: State<'_, Services>) -> Result<MetadataStatus> {
    let db = services.db.lock();
    Ok(MetadataStatus {
        has_key: services.credentials.get(CREDENTIAL_KEY).is_ok(),
        key_is_persistent: services.credentials.is_persistent(),
        movies: enrichment::coverage(&db, ItemKind::Movie)?,
        series: enrichment::coverage(&db, ItemKind::Series)?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyArgs {
    /// `None` clears the stored key.
    pub key: Option<String>,
}

#[tauri::command]
pub fn metadata_set_key(services: State<'_, Services>, args: SetKeyArgs) -> Result<()> {
    match args
        .key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        Some(key) => services
            .credentials
            .set(CREDENTIAL_KEY, &key)
            .map_err(|e| AppError::Other(e.to_string()))?,
        None => {
            // Clearing a key that was never set is success, not an error.
            let _ = services.credentials.delete(CREDENTIAL_KEY);
        }
    }
    Ok(())
}

fn client(services: &Services) -> Result<TmdbClient> {
    let key = services.credentials.get(CREDENTIAL_KEY).map_err(|_| {
        AppError::Other(
            "No metadata API key is set. Add one in Settings to fetch artwork and cast.".into(),
        )
    })?;
    Ok(TmdbClient::new(Arc::clone(&services.http), key))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    pub batch: Option<u32>,
    pub movies: Option<bool>,
    pub series: Option<bool>,
}

/// Enrich one batch, emitting `metadata.progress` as it goes.
///
/// One batch per call rather than looping to completion: a forty-thousand title library
/// would hold the connection for an hour, and the user can press the button again.
#[tauri::command]
pub fn metadata_run(
    app: tauri::AppHandle,
    services: State<'_, Services>,
    args: RunArgs,
) -> Result<Report> {
    let client = client(&services)?;
    let options = Options {
        batch: args.batch.unwrap_or(enrich::DEFAULT_BATCH),
        movies: args.movies.unwrap_or(true),
        series: args.series.unwrap_or(true),
    };

    let mut db = services.db.lock();
    let report = enrich::run(&mut db, &client, &options, now_unix(), |p| {
        // Best effort: a dropped progress event must never fail the pass.
        let _ = app.emit("metadata.progress", &p);
    })?;

    let _ = app.emit("metadata.done", &report);
    Ok(report)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsArgs {
    pub kind: String,
    pub id: i64,
}

fn parse_kind(s: &str) -> Result<ItemKind> {
    match s {
        "movie" => Ok(ItemKind::Movie),
        "series" => Ok(ItemKind::Series),
        other => Err(AppError::Other(format!("unknown item kind {other}"))),
    }
}

#[tauri::command]
pub fn metadata_credits(
    services: State<'_, Services>,
    args: CreditsArgs,
) -> Result<Vec<CreditRow>> {
    let kind = parse_kind(&args.kind)?;
    let db = services.db.lock();
    Ok(enrichment::credits_for(&db, kind, args.id)?)
}

/// Forget a title's match so the next pass looks again.
///
/// The escape hatch for a wrong match: the matcher declines when it is unsure, but it
/// can still be confidently wrong, and there has to be a way to say so.
#[tauri::command]
pub fn metadata_rematch(services: State<'_, Services>, args: CreditsArgs) -> Result<()> {
    let kind = parse_kind(&args.kind)?;
    let db = services.db.lock();
    enrichment::forget(&db, kind, args.id)?;
    enrichment::prune_people(&db)?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_ingest::credentials::{CredentialStore, MemoryStore};

    #[test]
    fn an_item_kind_is_validated_before_it_reaches_a_query() {
        assert_eq!(parse_kind("movie").unwrap(), ItemKind::Movie);
        assert_eq!(parse_kind("series").unwrap(), ItemKind::Series);
        let err = parse_kind("channel").unwrap_err();
        assert!(err.to_string().contains("channel"));
    }

    #[test]
    fn clearing_a_key_leaves_no_key_behind() {
        let store = MemoryStore::default();
        store.set(CREDENTIAL_KEY, "abcd1234").unwrap();
        store.delete(CREDENTIAL_KEY).unwrap();
        assert!(store.get(CREDENTIAL_KEY).is_err());

        // Clearing again is still fine here, but the Windows Credential Manager reports
        // a missing entry as an error — which is why the command ignores the result:
        // "there is no key" is the state the user asked for either way.
        let _ = store.delete(CREDENTIAL_KEY);
        assert!(store.get(CREDENTIAL_KEY).is_err());
    }

    #[test]
    fn a_stored_key_round_trips_but_only_through_the_credential_store() {
        let store = MemoryStore::default();
        store.set(CREDENTIAL_KEY, "abcd1234").unwrap();
        assert_eq!(store.get(CREDENTIAL_KEY).unwrap(), "abcd1234");
        // The in-memory store is honest about not surviving a restart, which is what
        // `keyIsPersistent` reports to the UI.
        assert!(!store.is_persistent());
    }
}
