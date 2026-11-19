//! Multi-view (README §7.4): what the Apple app needs from the core to put several
//! channels on screen at once.
//!
//! Windows' `aurora-app/src/mosaic.rs` keeps the whole session in the host, because the
//! tiles there are child windows the host positions. Here each tile is an mpv view
//! Swift owns, so Swift keeps the session (which tile has sound, whose turn it is in a
//! rotation) and the core keeps what must agree with Windows: the arithmetic of the
//! layouts (`aurora_core::mosaic`, shared), the connection budget and its refusal, a
//! tile's tune, and saved layouts (`aurora_db::repo::mosaic`, synced).

use aurora_core::mosaic::{Budget, Demand, Layout};
use aurora_db::repo::{channels, mosaic as repo};
use aurora_db::rusqlite::Connection;

use crate::error::{AuroraError, Result};
use aurora_video::request::Request;

use crate::playback::{live_sources, mpv_options, LiveTune, LIVE_CACHE_SECS};
use crate::{now_unix, AuroraCore};

/// How long each tile holds the single stream in a rotating mosaic, in seconds.
/// Windows' `DWELL_SECS`.
pub const DWELL_SECS: u32 = 12;

/// Where one tile goes, in the same units as the area it was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MosaicRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Windows' `MosaicCheck`: whether the provider's line can carry a layout.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MosaicCheck {
    pub layout: String,
    pub tiles: u32,
    /// `fits`, `exceeds` or `unknown`.
    pub verdict: String,
    pub needed: u32,
    /// `None` when no enabled provider has declared one (`unknown`).
    pub limit: Option<u32>,
    pub over: u32,
    /// The largest layout that would fit right now, or `None` when none does.
    pub largest_fitting: Option<String>,
    /// Whether this layout can be had as a rotation instead. True exactly when the
    /// budget refuses.
    pub rotation_offered: bool,
    /// Recordings in flight, which use the same line.
    pub recordings: u32,
}

/// A saved layout, as Windows' `SavedMosaicLayout`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SavedMosaicLayout {
    pub id: i64,
    pub name: String,
    pub layout: String,
    /// Channel id per tile, in tile order. `None` is an empty tile.
    pub channels: Vec<Option<i64>>,
    pub created_at: i64,
}

fn layout_of(name: &str) -> Result<Layout> {
    Layout::parse(name)
        .ok_or_else(|| AuroraError::other(format!("There is no layout called {name}.")))
}

/// The refusal, word for word Windows' `refusal`: the sentence this feature is judged on.
pub(crate) fn refusal(needed: usize, limit: usize, over: usize, recordings: usize) -> String {
    let streams = if needed == 1 { "stream" } else { "streams" };
    let mut msg = format!(
        "That needs {needed} {streams} at once and your provider allows {limit}. \
         Remove {over} channel{} and try again",
        if over == 1 { "" } else { "s" }
    );
    if recordings > 0 {
        let r = if recordings == 1 {
            "1 recording is".to_string()
        } else {
            format!("{recordings} recordings are")
        };
        msg.push_str(&format!(" — {r} using the line as well"));
    }
    msg.push('.');
    msg.push_str(
        " Or show them one at a time, each for a few seconds, on the single stream \
         your line allows.",
    );
    msg
}

/// What the picker should say about a layout (Windows' `Mosaic::check`).
pub(crate) fn check(layout: Layout, limit: Option<usize>, recordings: usize) -> MosaicCheck {
    let budget = Budget::check(
        Demand {
            tiles: layout.tiles(),
            recordings,
        },
        limit,
    );
    let (verdict, needed, limit, over) = match budget {
        Budget::Fits { needed, limit } => ("fits", needed, Some(limit), 0),
        Budget::Exceeds {
            needed,
            limit,
            over,
            ..
        } => ("exceeds", needed, Some(limit), over),
        Budget::Unknown { needed } => ("unknown", needed, None, 0),
    };
    MosaicCheck {
        layout: layout.as_str().into(),
        tiles: layout.tiles() as u32,
        verdict: verdict.into(),
        needed: needed as u32,
        limit: limit.map(|l| l as u32),
        over: over as u32,
        largest_fitting: limit
            .and_then(|l| Budget::largest_fitting(recordings, l))
            .map(|l| l.as_str().into()),
        rotation_offered: verdict == "exceeds",
        recordings: recordings as u32,
    }
}

/// Whether `streams` tiles may open alongside the recordings in flight: `Err` with
/// Windows' refusal when a limit the provider declared says no.
pub(crate) fn admit(streams: usize, limit: Option<usize>, recordings: usize) -> Result<()> {
    if let Budget::Exceeds {
        needed,
        limit,
        over,
        recordings,
    } = Budget::check(
        Demand {
            tiles: streams,
            recordings,
        },
        limit,
    ) {
        return Err(AuroraError::other(refusal(needed, limit, over, recordings)));
    }
    Ok(())
}

impl AuroraCore {
    fn line(&self, db: &Connection) -> (Option<usize>, usize) {
        (crate::dvr::line_limit(db), self.dvr.active_count())
    }
}

#[uniffi::export]
impl AuroraCore {
    /// What the picker should say about a layout before any channel is chosen
    /// (`mosaic.check`).
    pub fn mosaic_check(&self, layout: String) -> Result<MosaicCheck> {
        let layout = layout_of(&layout)?;
        let db = self.db.lock();
        let (limit, recordings) = self.line(&db);
        Ok(check(layout, limit, recordings))
    }

    /// Before opening `streams` tiles, each with its own stream: refused, in Windows'
    /// words, when that and the recordings in flight exceed a limit the provider
    /// declared. A rotating mosaic needs one stream whatever its size, so it asks with 1.
    /// `streams` of 0 is Windows' "Pick at least one channel".
    pub fn mosaic_admit(&self, streams: u32) -> Result<()> {
        if streams == 0 {
            return Err(AuroraError::other(
                "Pick at least one channel to show in the mosaic.",
            ));
        }
        let db = self.db.lock();
        let (limit, recordings) = self.line(&db);
        admit(streams as usize, limit, recordings)
    }

    /// Where each tile of `layout` goes in an area `width` by `height`, tile 0 first,
    /// with edges shared between neighbours so the picture has no seams. The same numbers
    /// Windows positions its tiles by.
    pub fn mosaic_rects(&self, layout: String, width: u32, height: u32) -> Result<Vec<MosaicRect>> {
        Ok(layout_of(&layout)?
            .rects(width, height)
            .into_iter()
            .map(|r| MosaicRect {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            })
            .collect())
    }

    /// How long each tile holds the stream in a rotation.
    pub fn mosaic_dwell_secs(&self) -> u32 {
        DWELL_SECS
    }

    /// One tile's channel, as `tune_live` but without the rewind buffer: nine on-disk
    /// ring buffers is a disk fire and nobody rewinds a tile (Windows' `build_tile`).
    pub fn tune_tile(&self, channel_id: i64) -> Result<LiveTune> {
        let db = self.db.lock();
        crate::parental::allowed(&db, aurora_db::repo::parental::Target::Channel(channel_id))?;
        let channel = channels::get(&db, channel_id)?.ok_or_else(|| {
            AuroraError::not_found(
                "That channel is no longer in the library. A refresh may have removed it.",
            )
        })?;
        let sources = live_sources(&db, self.credentials.as_ref(), channel_id)?;
        if sources.is_empty() {
            return Err(AuroraError::other(
                "Your provider listed this channel without a stream.",
            ));
        }
        let options = Request {
            live: true,
            cache_secs: LIVE_CACHE_SECS,
            timeshift: None,
            ..Default::default()
        };
        Ok(LiveTune {
            channel_id,
            title: channel.name,
            sources,
            mpv_options: mpv_options(&options),
            rewind: None,
        })
    }

    /// Saved layouts, newest first (`mosaic.layouts`). One saved by a newer Aurora with
    /// an arrangement this one does not know is left out rather than shown broken.
    pub fn mosaic_layouts(&self) -> Result<Vec<SavedMosaicLayout>> {
        let rows = repo::list_layouts(&self.db.lock())?;
        Ok(rows
            .into_iter()
            .filter(|r| Layout::parse(&r.layout).is_some())
            .map(|r| SavedMosaicLayout {
                id: r.id,
                name: r.name,
                layout: r.layout,
                channels: r.channels,
                created_at: r.created_at,
            })
            .collect())
    }

    /// Save the mosaic as it stands (`mosaic.save`). Saving over a name replaces it.
    pub fn mosaic_save(
        &self,
        name: String,
        layout: String,
        channels: Vec<Option<i64>>,
    ) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AuroraError::other("Give the layout a name."));
        }
        let layout = layout_of(&layout)?;
        let mut channels = channels;
        channels.truncate(layout.tiles());
        channels.resize(layout.tiles(), None);
        Ok(repo::upsert_layout(
            &self.db.lock(),
            name,
            layout.as_str(),
            &channels,
            now_unix(),
        )?)
    }

    pub fn mosaic_delete_layout(&self, id: i64) -> Result<bool> {
        Ok(repo::delete_layout(&self.db.lock(), id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_stream_line_refuses_every_layout_and_offers_a_rotation() {
        let c = check(Layout::Grid2x2, Some(1), 0);
        assert_eq!(c.verdict, "exceeds");
        assert_eq!((c.needed, c.limit, c.over), (4, Some(1), 3));
        assert!(c.rotation_offered);
        assert_eq!(c.largest_fitting, None);

        let err = admit(4, Some(1), 0).unwrap_err().to_string();
        assert!(
            err.contains("That needs 4 streams at once and your provider allows 1."),
            "{err}"
        );
        assert!(err.contains("Remove 3 channels and try again."), "{err}");
        assert!(err.contains("Or show them one at a time"), "{err}");
        // A rotation asks for one stream, and one fits.
        assert!(admit(1, Some(1), 0).is_ok());
    }

    #[test]
    fn recordings_count_against_the_line_and_are_named() {
        let c = check(Layout::Grid2x2, Some(4), 1);
        assert_eq!(c.verdict, "exceeds");
        assert_eq!(c.recordings, 1);
        assert_eq!(c.largest_fitting, None);
        let err = admit(4, Some(4), 1).unwrap_err().to_string();
        assert!(
            err.contains("Remove 1 channel and try again — 1 recording is using the line as well."),
            "{err}"
        );

        let fits = check(Layout::Grid2x2, Some(9), 0);
        assert_eq!(fits.verdict, "fits");
        assert_eq!(fits.largest_fitting.as_deref(), Some("grid3x3"));
    }

    #[test]
    fn no_declared_limit_is_unknown_not_fine() {
        let c = check(Layout::Grid3x3, None, 0);
        assert_eq!(c.verdict, "unknown");
        assert_eq!(c.limit, None);
        assert!(!c.rotation_offered);
        assert_eq!(c.largest_fitting, None);
        assert!(admit(9, None, 0).is_ok());
    }
}
