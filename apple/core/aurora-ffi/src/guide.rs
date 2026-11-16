//! The programme guide: what is on now and next, and one channel's schedule.
//!
//! Straight reads of `aurora_db::repo::epg`. A channel's guide is keyed by the EPG id
//! the matcher gave it during import, so these take Aurora channel ids — what Swift has —
//! and translate.

use aurora_db::repo::{channels, epg};

use crate::error::Result;
use crate::{now_unix, AuroraCore};

/// One guide entry.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Programme {
    pub id: i64,
    /// Unix seconds.
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub sub_title: Option<String>,
    pub description: Option<String>,
    pub categories: Vec<String>,
    pub icon: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub rating: Option<String>,
    pub is_new: bool,
    pub is_live: bool,
}

impl From<epg::ProgrammeRow> for Programme {
    fn from(p: epg::ProgrammeRow) -> Self {
        Self {
            id: p.id,
            start: p.start,
            stop: p.stop,
            title: p.title,
            sub_title: p.sub_title,
            description: p.description,
            categories: p.categories,
            icon: p.icon,
            season: p.season,
            episode: p.episode,
            rating: p.rating,
            is_new: p.is_new,
            is_live: p.is_live,
        }
    }
}

/// What a channel row shows under its name.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct NowNext {
    pub channel_id: i64,
    pub now: Option<Programme>,
    pub next: Option<Programme>,
}

#[uniffi::export]
impl AuroraCore {
    /// Now and next for each channel asked about, in the same order. A channel with no
    /// guide gets both empty rather than being left out, so Swift can zip the answer
    /// with what it asked.
    pub fn now_next(&self, channel_ids: Vec<i64>) -> Result<Vec<NowNext>> {
        let db = self.db.lock();
        let now = now_unix();
        let mut out = Vec::with_capacity(channel_ids.len());
        for channel_id in channel_ids {
            let guide_id = channels::get(&db, channel_id)?.and_then(|c| c.epg_channel_id);
            let (current, next) = match guide_id {
                Some(id) => epg::now_next(&db, &id, now)?,
                None => (None, None),
            };
            out.push(NowNext {
                channel_id,
                now: current.map(Programme::from),
                next: next.map(Programme::from),
            });
        }
        Ok(out)
    }

    /// One channel's programmes overlapping `from..to` (Unix seconds), in order. The
    /// window is capped at 48 hours, as on Windows (`epg::MAX_SLICE_SECS`).
    pub fn schedule(&self, channel_id: i64, from: i64, to: i64) -> Result<Vec<Programme>> {
        let db = self.db.lock();
        let Some(guide_id) = channels::get(&db, channel_id)?.and_then(|c| c.epg_channel_id) else {
            return Ok(Vec::new());
        };
        Ok(epg::grid_slice(&db, &[guide_id], from, to)?
            .into_iter()
            .map(Programme::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::core;

    #[test]
    fn channels_with_no_guide_still_get_an_answer_each() {
        let (core, _dir) = core();
        let answer = core.now_next(vec![7, 8]).unwrap();
        assert_eq!(answer.len(), 2);
        assert_eq!(answer[0].channel_id, 7);
        assert!(answer[1].now.is_none() && answer[1].next.is_none());
    }

    #[test]
    fn an_unknown_channel_has_an_empty_schedule() {
        let (core, _dir) = core();
        assert!(core.schedule(1, 0, 3600).unwrap().is_empty());
    }
}
