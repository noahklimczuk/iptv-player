//! What was on a channel at a moment: for the sleep timer (H19), the seek preview's
//! programme name (H25) and Record (H26).

use aurora_db::repo::{channels, epg};
use aurora_db::rusqlite::Connection;

use crate::error::Result;

/// The programme on `channel_id` at `at` (Unix seconds), if the guide knows it. A
/// channel the provider never matched to a listing has none, which is ordinary.
pub fn on_at(db: &Connection, channel_id: i64, at: i64) -> Result<Option<epg::ProgrammeRow>> {
    let Some(ch) = channels::get(db, channel_id)? else {
        return Ok(None);
    };
    let Some(id) = ch.epg_channel_id.filter(|id| !id.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(epg::now_next(db, &id, at)?.0)
}
