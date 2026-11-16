//! Turning a channel into something MPVKit can open.
//!
//! The player itself is Swift (MPVKit in a SwiftUI view), but what it is told stays
//! here. [`LiveTune::mpv_options`] is `aurora_player::backend::LoadOptions::mpv_options`
//! — the same function that configures mpv on Windows — so probe sizes, cache depth and
//! the User-Agent are identical on both platforms. Swift sets each pair with
//! `mpv_set_option_string` before `loadfile`.
//!
//! Mirrors `aurora-app/src/window.rs` `live_sources` and `aurora-app/src/streams.rs`
//! `playable`. The failover loop around them (`aurora-app/src/playback.rs`) is not here
//! yet: Swift tries [`LiveTune::sources`] in order and reports each outcome through
//! [`AuroraCore::report_source`], which feeds the same ranking Windows uses.

use std::collections::HashMap;

use aurora_core::model::MediaKind;
use aurora_db::repo::{channels, sources};
use aurora_db::rusqlite::{params, Connection, OptionalExtension};
use aurora_ingest::credentials::CredentialStore;
use aurora_player::backend::{LoadOptions, Playing};

use crate::error::{AuroraError, Result};
use crate::{now_unix, AuroraCore};

/// Network buffer for live TV, in seconds (README §6.1).
///
/// Mirrors `aurora-app/src/window.rs` `cache_secs_for(true)`; `mirrors.rs` fails
/// if the two disagree.
pub(crate) const LIVE_CACHE_SECS: u32 = 8;

/// One address a channel can be played from.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlayableSource {
    /// Pass back to [`AuroraCore::report_source`].
    pub source_id: i64,
    /// Carries the provider's credentials for an Xtream panel. Hand it to the player;
    /// never log it (README C10).
    pub url: String,
}

/// One mpv option, as `name=value`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MpvOption {
    pub name: String,
    pub value: String,
}

/// Everything needed to start a live channel.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LiveTune {
    pub channel_id: i64,
    pub title: String,
    /// Best bet first. Try the next one when a source fails to open.
    pub sources: Vec<PlayableSource>,
    /// Set on mpv before loading any of `sources`.
    pub mpv_options: Vec<MpvOption>,
}

#[uniffi::export]
impl AuroraCore {
    /// Everything needed to play a live channel.
    pub fn tune_live(&self, channel_id: i64) -> Result<LiveTune> {
        let db = self.db.lock();
        let channel = channels::get(&db, channel_id)?.ok_or_else(|| {
            AuroraError::not_found(
                "That channel is no longer in the library. A refresh may have removed it.",
            )
        })?;

        let mut access = HashMap::new();
        let mut playable = Vec::new();
        let mut last_refusal = None;
        for source in sources::for_channel(&db, channel_id, now_unix())? {
            match stream_url(
                &db,
                self.credentials.as_ref(),
                &mut access,
                MediaKind::Live,
                &StreamRef::from(&source),
            ) {
                Ok(url) if !url.trim().is_empty() => playable.push(PlayableSource {
                    source_id: source.id,
                    url,
                }),
                Ok(_) => {}
                // One source that cannot be rebuilt is no reason to refuse the others.
                Err(e) => last_refusal = Some(e),
            }
        }
        if playable.is_empty() {
            return Err(last_refusal.unwrap_or_else(|| {
                AuroraError::other(
                    "Your provider listed this channel without a playable address, so \
                     there is nothing to open. A library refresh may fill it in.",
                )
            }));
        }

        let options = LoadOptions {
            is_live: true,
            cache_secs: LIVE_CACHE_SECS,
            title: Some(channel.name.clone()),
            playing: Some(Playing {
                kind: aurora_player::state::MediaKind::Live,
                id: channel_id,
                channel_id: Some(channel_id),
            }),
            ..Default::default()
        };

        Ok(LiveTune {
            channel_id,
            title: channel.name,
            sources: playable,
            mpv_options: mpv_options(&options),
        })
    }

    /// Say whether a source opened. The next tune of the channel starts from the one
    /// that worked, exactly as on Windows (`aurora_db::repo::sources`).
    pub fn report_source(&self, source_id: i64, opened: bool) -> Result<()> {
        let db = self.db.lock();
        if opened {
            sources::record_ok(&db, source_id, now_unix())?;
        } else {
            sources::record_failure(&db, source_id, now_unix())?;
        }
        Ok(())
    }
}

/// What a stored row says about where to stream it from.
///
/// Mirrors `aurora-app/src/streams.rs` `StreamRef`.
pub(crate) struct StreamRef {
    pub url: String,
    pub stream_id: Option<String>,
    pub container: Option<String>,
    pub provider_id: Option<i64>,
}

impl From<&sources::Source> for StreamRef {
    fn from(s: &sources::Source) -> Self {
        Self {
            url: s.url.clone(),
            stream_id: s.stream_id.clone(),
            container: s.container.clone(),
            provider_id: Some(s.provider_id),
        }
    }
}

/// A playable address: the stored URL, or one rebuilt from the stream id and the
/// provider's Keychain password (R-126). Empty when the row has neither, which callers
/// turn into a sentence naming what is missing.
///
/// Mirrors `aurora-app/src/streams.rs` `playable` and `provider_access`.
pub(crate) fn stream_url(
    db: &Connection,
    credentials: &dyn CredentialStore,
    cache: &mut HashMap<i64, (String, String, String)>,
    kind: MediaKind,
    stream: &StreamRef,
) -> Result<String> {
    if !stream.url.trim().is_empty() {
        return Ok(stream.url.clone());
    }
    let Some(stream_id) = stream.stream_id.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(String::new());
    };
    let provider_id = stream.provider_id.ok_or_else(|| {
        AuroraError::other(
            "This title is not attached to a provider any more, so its stream address \
             cannot be rebuilt. A refresh should restore it.",
        )
    })?;

    let (base_url, username, password) = match cache.get(&provider_id) {
        Some(access) => access.clone(),
        None => {
            let access = provider_access(db, credentials, provider_id)?;
            cache.insert(provider_id, access.clone());
            access
        }
    };

    let id: u32 = stream_id.parse().map_err(|_| {
        AuroraError::other(format!(
            "This title's stream id ({stream_id}) is not a number, so its address \
             cannot be rebuilt. A refresh should restore it."
        ))
    })?;
    Ok(aurora_core::xtream::stream_url(
        &base_url,
        &username,
        &password,
        kind,
        id,
        stream.container.as_deref(),
    ))
}

/// The options shared by every tune, as Swift applies them.
pub(crate) fn mpv_options(options: &LoadOptions) -> Vec<MpvOption> {
    options
        .mpv_options()
        .into_iter()
        .map(|(name, value)| MpvOption { name, value })
        .collect()
}

fn provider_access(
    db: &Connection,
    credentials: &dyn CredentialStore,
    provider_id: i64,
) -> Result<(String, String, String)> {
    let row: Option<(String, String, Option<String>)> = db
        .query_row(
            "SELECT base_url, COALESCE(username, ''), credential_ref
             FROM providers WHERE id = ?1",
            params![provider_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (base_url, username, key) = row.ok_or_else(|| {
        AuroraError::not_found(
            "The provider this title came from is no longer set up, so its stream \
             address cannot be rebuilt.",
        )
    })?;
    let key = key.ok_or_else(|| {
        AuroraError::credentials(
            "This provider has no saved password, so Aurora cannot build the stream \
             address. Edit the provider and enter it again.",
        )
    })?;
    let password = credentials.get(&key).map_err(|e| {
        AuroraError::credentials(format!(
            "Aurora could not read this provider's saved password: {e}. Edit the \
             provider and enter it again."
        ))
    })?;
    Ok((base_url, username, password))
}

#[cfg(test)]
mod tests {
    use crate::testing::core;

    #[test]
    fn an_unknown_channel_is_not_found_rather_than_a_panic() {
        let (core, _dir) = core();
        assert!(matches!(
            core.tune_live(9_999),
            Err(crate::AuroraError::NotFound { .. })
        ));
    }

    /// The mpv options come from the shared `LoadOptions`, so a live tune on Apple is
    /// configured exactly as one on Windows: low-latency probing and a User-Agent.
    #[test]
    fn live_options_are_the_shared_ones() {
        let shared = aurora_player::backend::LoadOptions {
            is_live: true,
            cache_secs: super::LIVE_CACHE_SECS,
            ..Default::default()
        }
        .mpv_options();
        assert!(shared.iter().any(|(k, _)| k == "user-agent"));
    }
}
