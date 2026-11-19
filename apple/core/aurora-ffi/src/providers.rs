//! Adding a provider and importing what it offers.
//!
//! Mirrors `aurora-app/src/providers.rs` — `save_provider` and `refresh_provider` — and
//! nothing else from it. The import itself is `aurora_ingest::sync`, shared and unchanged.
//!
//! **Kept in step by hand.** If `refresh_provider` on Windows gains a step that changes
//! what is stored (as `backfill_stream_ids` once did), this file needs the same step, or
//! an Apple library and a Windows one built from the same panel will differ.

use std::sync::Arc;

use aurora_core::rules::{Rule, RuleSet};
use aurora_db::rusqlite::{params, OptionalExtension};
use aurora_ingest::credentials::{credential_ref, namespace};
use aurora_ingest::source::SourceKind;
use aurora_ingest::sync::{self, Phase, Progress, SyncOptions};

use crate::error::{AuroraError, Result};
use crate::{now_unix, AuroraCore};

/// The guides `aurora-app` adds for channels that name Pluto TV. Same list, same scope,
/// so a Pluto channel has the same guide on the TV as on the PC.
///
/// Mirrors `aurora-app/src/providers.rs` `FREE_GUIDES` and `FREE_GUIDES_KEY`;
/// `mirrors.rs` fails if they disagree.
pub(crate) const FREE_GUIDES: &[&str] = &[
    "https://i.mjh.nz/PlutoTV/us.xml.gz",
    "https://i.mjh.nz/PlutoTV/ca.xml.gz",
];
pub(crate) const FREE_GUIDES_KEY: &str = "guide.free_sources";

/// A provider, with what it contributed to the library.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    /// `"xtream"` or `"m3u"`.
    pub kind: String,
    pub enabled: bool,
    /// Streams at once this line allows, once a refresh has asked.
    pub max_connections: Option<i64>,
    pub last_refresh_at: Option<i64>,
    pub channel_count: i64,
    pub movie_count: i64,
    pub series_count: i64,
}

/// What an import should bring in.
///
/// On Apple TV this matters more than on a PC: a 117k-film catalogue and its search
/// index on a device whose storage the system can purge is docs/APPLE_PORT_AND_SYNC.md
/// risk 3. Live only is a reasonable first import on tvOS.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct ImportContent {
    pub live: bool,
    pub vod: bool,
    pub series: bool,
}

/// Where an import has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ImportPhase {
    Authenticating,
    FetchingPlaylist,
    ImportingChannels,
    ImportingMovies,
    ImportingSeries,
    FetchingEpg,
    MatchingEpg,
    Indexing,
    Done,
}

impl From<Phase> for ImportPhase {
    fn from(p: Phase) -> Self {
        match p {
            Phase::Authenticating => Self::Authenticating,
            Phase::FetchingPlaylist => Self::FetchingPlaylist,
            Phase::ImportingChannels => Self::ImportingChannels,
            Phase::ImportingMovies => Self::ImportingMovies,
            Phase::ImportingSeries => Self::ImportingSeries,
            Phase::FetchingEpg => Self::FetchingEpg,
            Phase::MatchingEpg => Self::MatchingEpg,
            Phase::Indexing => Self::Indexing,
            Phase::Done => Self::Done,
        }
    }
}

/// Told about an import as it goes, to draw a progress bar. Implemented in Swift.
///
/// Called from the importing thread, often. Swift hops to the main actor itself.
#[uniffi::export(with_foreign)]
pub trait ImportListener: Send + Sync {
    /// `total` is zero while it cannot be known yet, e.g. while a guide streams in.
    fn progress(&self, phase: ImportPhase, done: u64, total: u64);
}

/// What an import found. README §4.6: "142 new movies, 6 new episodes… 3 channels
/// removed".
#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportReport {
    pub channels: u64,
    pub movies: u64,
    pub series: u64,
    pub episodes: u64,
    pub epg_programmes: u64,
    pub epg_matched: u64,
    /// Channels with no guide, all of them: the setup wizard's coverage figure divides by
    /// this plus `epg_matched` (Windows' `epgUnmatchedTotal`, R-121).
    pub epg_unmatched_total: u64,
    /// Channels the provider stopped listing. Kept and flagged, never deleted.
    pub channels_missing: u64,
    /// Entries the provider listed that were not kept, for any reason.
    pub dropped: u64,
    pub warnings: Vec<String>,
}

impl From<sync::SyncReport> for ImportReport {
    fn from(r: sync::SyncReport) -> Self {
        Self {
            channels: r.channels as u64,
            movies: r.movies as u64,
            series: r.series as u64,
            episodes: r.episodes as u64,
            epg_programmes: r.epg_programmes as u64,
            epg_matched: r.epg_matched as u64,
            epg_unmatched_total: r.epg_unmatched_total as u64,
            channels_missing: r.channels_missing as u64,
            dropped: r.dropped.total() as u64,
            warnings: r.warnings,
        }
    }
}

#[uniffi::export]
impl AuroraCore {
    /// Save an Xtream Codes panel. Imports nothing: call [`AuroraCore::refresh`] next.
    pub fn add_xtream(
        &self,
        name: String,
        base_url: String,
        username: String,
        password: String,
    ) -> Result<i64> {
        self.save_provider(
            &name,
            "xtream",
            base_url.trim(),
            Some(username.trim()),
            Some(password),
        )
    }

    /// Save a playlist URL. Imports nothing: call [`AuroraCore::refresh`] next.
    pub fn add_m3u(&self, name: String, url: String) -> Result<i64> {
        self.save_provider(&name, "m3u", url.trim(), None, None)
    }

    /// Remove a provider: its channels, films, series and guide go with it (the schema
    /// cascades), and its password leaves the Keychain. Returns false if it was already
    /// gone. Mirrors `aurora-app/src/providers.rs` `providers_delete`.
    pub fn remove_provider(&self, provider_id: i64) -> Result<bool> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let db = self.db.lock();
        let credential: Option<String> = db
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = ?1",
                params![provider_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let removed = db.execute("DELETE FROM providers WHERE id = ?1", params![provider_id])?;
        if removed == 0 {
            return Ok(false);
        }
        if let Some(key) = credential {
            // Already forgotten is fine: the point is that nothing is left behind.
            let _ = self.credentials.delete(&key);
        }
        Ok(true)
    }

    /// Every provider, in the order they were added.
    pub fn providers(&self) -> Result<Vec<Provider>> {
        let db = self.db.lock();
        Ok(aurora_db::repo::providers::list(&db)?
            .into_iter()
            .map(|p| Provider {
                id: p.id,
                name: p.name,
                kind: p.kind,
                enabled: p.enabled,
                max_connections: p.max_connections,
                last_refresh_at: p.last_refresh_at,
                channel_count: p.channel_count,
                movie_count: p.movie_count,
                series_count: p.series_count,
            })
            .collect())
    }

    /// Download and import everything `content` asks for from one provider.
    ///
    /// Blocks for as long as the import takes. The database is held only for the
    /// writes, not the download, as on Windows.
    ///
    /// Mirrors `aurora-app/src/providers.rs` `refresh_provider`, less what the spike has
    /// no use for yet: metadata enrichment and the episode sweep (later phases).
    pub fn refresh(
        &self,
        provider_id: i64,
        content: ImportContent,
        listener: Arc<dyn ImportListener>,
    ) -> Result<ImportReport> {
        let (kind, base_url, username, credential_ref_value) = {
            let db = self.db.lock();
            db.query_row(
                "SELECT kind, base_url, COALESCE(username, ''), credential_ref
                 FROM providers WHERE id = ?1",
                params![provider_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AuroraError::not_found(format!("There is no provider {provider_id}.")))?
        };

        let source = match kind.as_str() {
            "xtream" => SourceKind::Xtream { base_url, username },
            _ => SourceKind::M3u { url: base_url },
        };

        let mut options = SyncOptions::new(provider_id, source, now_unix());
        options.password = match credential_ref_value {
            None => None,
            Some(key) => Some(self.credentials.get(&key).map_err(|e| {
                AuroraError::credentials(format!(
                    "Aurora could not read this provider's saved password: {e}. Edit the \
                     provider and enter it again."
                ))
            })?),
        };
        options.import_live = content.live;
        options.import_vod = content.vod;
        options.import_series = content.series;

        let rules = {
            let db = self.db.lock();
            if aurora_db::repo::settings::get_or(&db, FREE_GUIDES_KEY, true)? {
                options.scoped_epg = FREE_GUIDES
                    .iter()
                    .map(|url| sync::ScopedGuide {
                        url: (*url).to_string(),
                        only_where: "pluto".into(),
                    })
                    .collect();
            }
            if aurora_db::repo::filtering::LibraryFilter::load(&db)?.english_only {
                options.epg_title_langs = vec!["en".into()];
            }
            load_rules(&db)?
        };

        let report = |p: Progress| listener.progress(p.phase.into(), p.done as u64, p.total as u64);

        // No lock across the download.
        let fetched = sync::fetch(&self.http, &options, &rules, report)?;
        let max_connections = fetched
            .account()
            .and_then(|a| a.max_connections)
            .map(|n| n as i64);

        let mut db = self.db.lock();
        let result = sync::apply(&mut db, fetched, &options, report)?;

        // `options.now_unix`, the instant every row was stamped with, so a channel is
        // not shown as dropped by the refresh that just listed it. See the long comment
        // at the same line of `refresh_provider`.
        db.execute(
            "UPDATE providers
                SET last_refresh_at = ?2,
                    max_connections = COALESCE(?3, max_connections)
              WHERE id = ?1",
            params![provider_id, options.now_unix, max_connections],
        )?;

        // Take the credentials straight back out of the stored stream URLs (R-126).
        match aurora_db::backfill_stream_ids(&db) {
            Ok(0) => {}
            Ok(n) => tracing::info!(rows = n, "stored stream ids in place of stream URLs"),
            Err(e) => tracing::error!("could not rewrite stored stream URLs: {e}"),
        }

        // Fresh guide data is the moment a series rule can find new episodes, so the
        // schedule is right as soon as the import is (`refresh_provider` does the same).
        match crate::dvr::expand_rules_now(&db, now_unix()) {
            Ok(added) if !added.is_empty() => {
                tracing::info!("series rules scheduled {} new recordings", added.len());
            }
            Ok(_) => {}
            // A rule that could not be expanded must not fail the import the user waited for.
            Err(e) => tracing::warn!("could not expand series rules after refresh: {e}"),
        }
        drop(db);

        // Records from other devices that named something this library did not have yet
        // are parked; an import is when they may finally have somewhere to go
        // (`refresh_provider` does the same).
        self.sync.wake();

        Ok(result.into())
    }
}

impl AuroraCore {
    /// Mirrors `aurora-app/src/providers.rs` `save_provider`.
    fn save_provider(
        &self,
        name: &str,
        kind: &str,
        url: &str,
        username: Option<&str>,
        password: Option<String>,
    ) -> Result<i64> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        if url.is_empty() {
            return Err(AuroraError::other("Enter the provider's address."));
        }
        let name = if name.trim().is_empty() {
            url
        } else {
            name.trim()
        };
        let password = password.filter(|p| !p.is_empty());

        let db = self.db.lock();
        // Ids are never reused, so the key minted for the next id names the row the
        // insert is about to take.
        let next_id: i64 =
            db.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM providers", [], |r| {
                r.get(0)
            })?;
        let key = credential_ref(&namespace(&db)?, next_id);

        if let Some(password) = &password {
            self.credentials.set(&key, password)?;
        }

        let stored = password.as_ref().map(|_| key.as_str());
        let inserted = db.execute(
            "INSERT INTO providers (id, name, kind, base_url, username, credential_ref,
                                    enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
            params![next_id, name, kind, url, username, stored, now_unix()],
        );
        if let Err(e) = inserted {
            // Nothing was saved, so no secret should be left behind under a key no
            // provider references (README C10).
            if password.is_some() {
                let _ = self.credentials.delete(&key);
            }
            return Err(e.into());
        }
        Ok(next_id)
    }
}

/// Mirrors `aurora-app/src/providers.rs` `load_rules`.
fn load_rules(db: &aurora_db::rusqlite::Connection) -> Result<RuleSet> {
    let mut stmt =
        db.prepare("SELECT definition FROM rules WHERE enabled = 1 ORDER BY sort_order")?;
    let defs = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let rules: Vec<Rule> = defs
        .iter()
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect();
    Ok(RuleSet::compile(&rules)?)
}

#[cfg(test)]
mod tests {
    use crate::testing::core;

    #[test]
    fn an_added_provider_is_listed_with_an_empty_library() {
        let (core, _dir) = core();
        let id = core
            .add_xtream(
                "Panel".into(),
                "http://panel.example:8080".into(),
                "alice".into(),
                "secret".into(),
            )
            .unwrap();
        let all = core.providers().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
        assert_eq!(all[0].kind, "xtream");
        assert_eq!(all[0].channel_count, 0);
    }

    /// README C10: the password goes to the Keychain, and the library holds only the
    /// key that names it.
    #[test]
    fn the_password_is_in_the_keychain_and_not_in_the_library() {
        let (core, _dir) = core();
        let id = core
            .add_xtream(
                "Panel".into(),
                "http://panel.example".into(),
                "alice".into(),
                "hunter2".into(),
            )
            .unwrap();
        let key: String = core
            .db
            .lock()
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!key.contains("hunter2"));
        assert_eq!(core.credentials.get(&key).unwrap(), "hunter2");
    }

    #[test]
    fn removing_a_provider_forgets_its_password() {
        let (core, _dir) = core();
        let id = core
            .add_xtream(
                "Panel".into(),
                "http://panel.example".into(),
                "alice".into(),
                "hunter2".into(),
            )
            .unwrap();
        let key: String = core
            .db
            .lock()
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(core.remove_provider(id).unwrap());
        assert!(core.providers().unwrap().is_empty());
        assert!(
            core.credentials.get(&key).is_err(),
            "the password outlived its provider"
        );
        assert!(
            !core.remove_provider(id).unwrap(),
            "a second remove finds nothing"
        );
    }

    #[test]
    fn an_m3u_provider_has_no_password() {
        let (core, _dir) = core();
        let id = core
            .add_m3u("List".into(), "http://lists.example/tv.m3u".into())
            .unwrap();
        let key: Option<String> = core
            .db
            .lock()
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(key, None);
    }

    #[test]
    fn a_blank_address_is_refused_in_words() {
        let (core, _dir) = core();
        let e = core.add_m3u("List".into(), "  ".into()).unwrap_err();
        assert!(e.to_string().contains("address"));
    }

    #[test]
    fn refreshing_a_provider_that_is_not_there_says_so() {
        struct Quiet;
        impl super::ImportListener for Quiet {
            fn progress(&self, _: super::ImportPhase, _: u64, _: u64) {}
        }
        let (core, _dir) = core();
        let e = core
            .refresh(
                42,
                super::ImportContent {
                    live: true,
                    vod: false,
                    series: false,
                },
                std::sync::Arc::new(Quiet),
            )
            .unwrap_err();
        assert!(matches!(e, crate::AuroraError::NotFound { .. }));
    }
}
