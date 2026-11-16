//! Catch-up: playing a programme that has already aired, from the guide.
//!
//! Which programmes can be replayed, and the URL that replays one, are decided by the
//! shared `aurora_core::catchup` (`Mode::parse`, `is_available`, `url_for`). This module
//! only finds the channel's stream and catch-up settings, and mirrors
//! `aurora-app/src/window.rs` `catchup_channel` and `resolve_catchup`.

use std::collections::HashMap;

use aurora_core::catchup;
use aurora_core::model::MediaKind;
use aurora_db::rusqlite::OptionalExtension;
use aurora_player::backend::{LoadOptions, Playing};

use crate::error::{AuroraError, Result};
use crate::playback::{mpv_options, stream_url, MpvOption, StreamRef};
use crate::vod::VOD_CACHE_SECS;
use crate::{now_unix, AuroraCore};

/// Everything needed to replay one programme.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CatchupTune {
    pub channel_id: i64,
    pub title: String,
    /// Can carry the provider's credentials: hand it to the player, never log it.
    pub url: String,
    pub mpv_options: Vec<MpvOption>,
}

#[uniffi::export]
impl AuroraCore {
    /// For each programme start time on this channel, whether it can be replayed now,
    /// in the same order. For drawing a schedule: a replay button only where one will
    /// work, decided by the shared rule (`catchup::is_available`) rather than in Swift.
    pub fn catch_up_available(&self, channel_id: i64, starts: Vec<i64>) -> Result<Vec<bool>> {
        let db = self.db.lock();
        let row: Option<(Option<String>, i64)> = db
            .query_row(
                "SELECT catchup_mode, catchup_days FROM channels WHERE id = ?1",
                [channel_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let days = match row {
            Some((Some(mode), days)) if catchup::Mode::parse(&mode).is_some() => {
                days.clamp(0, i64::from(u16::MAX)) as u16
            }
            _ => return Ok(vec![false; starts.len()]),
        };
        let now = now_unix();
        Ok(starts
            .into_iter()
            .map(|start| catchup::is_available(start, now, days))
            .collect())
    }

    /// Everything needed to replay the programme on `channel_id` that ran `start..stop`
    /// (Unix seconds).
    pub fn tune_catchup(&self, channel_id: i64, start: i64, stop: i64) -> Result<CatchupTune> {
        let now = now_unix();
        let db = self.db.lock();

        // The channel's best source, as `catchup_channel` picks it.
        let (name, mode, source, days, stream) = db
            .query_row(
                "SELECT c.name, c.catchup_mode, c.catchup_source, c.catchup_days,
                        s.url, s.stream_id, s.container, c.provider_id
                 FROM channels c
                 LEFT JOIN channel_sources s ON s.channel_id = c.id
                 WHERE c.id = ?1
                 ORDER BY s.priority, s.fail_count
                 LIMIT 1",
                [channel_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, i64>(3)?,
                        StreamRef {
                            url: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                            stream_id: r.get(5)?,
                            container: r.get(6)?,
                            provider_id: r.get(7)?,
                        },
                    ))
                },
            )
            .map_err(|_| {
                AuroraError::not_found(
                    "That channel is no longer in the library. A refresh may have removed it.",
                )
            })?;
        let days = days.clamp(0, i64::from(u16::MAX)) as u16;

        let Some(mode) = mode.as_deref().and_then(catchup::Mode::parse) else {
            return Err(AuroraError::other(format!(
                "{name} does not offer catch-up"
            )));
        };
        if !catchup::is_available(start, now, days) {
            return Err(AuroraError::other(if start > now {
                format!("{name} has not aired yet")
            } else {
                format!("That programme is outside {name}'s {days}-day catch-up window")
            }));
        }

        let live_url = if stream.url.is_empty() && stream.stream_id.is_none() {
            String::new()
        } else {
            stream_url(
                &db,
                self.credentials.as_ref(),
                &mut HashMap::new(),
                MediaKind::Live,
                &stream,
            )?
        };

        let url = catchup::url_for(&catchup::Request {
            stream_url: &live_url,
            mode,
            source: source.as_deref(),
            start,
            stop,
            now,
        })
        .ok_or_else(|| {
            AuroraError::other(format!(
                "{name} says it has catch-up but not how to request it"
            ))
        })?;

        let options = LoadOptions {
            is_live: false,
            cache_secs: VOD_CACHE_SECS,
            title: Some(name.clone()),
            playing: Some(Playing {
                kind: aurora_player::state::MediaKind::Live,
                id: channel_id,
                channel_id: Some(channel_id),
            }),
            ..Default::default()
        };
        Ok(CatchupTune {
            channel_id,
            title: name,
            url,
            mpv_options: mpv_options(&options),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::core;

    #[test]
    fn an_unknown_channel_cannot_catch_up() {
        let (core, _dir) = core();
        assert_eq!(
            core.catch_up_available(1, vec![0, 1]).unwrap(),
            [false, false]
        );
        assert!(matches!(
            core.tune_catchup(1, 0, 60),
            Err(crate::AuroraError::NotFound { .. })
        ));
    }
}
