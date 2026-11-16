//! Reading the library: channels and their groups.
//!
//! Straight calls into `aurora_db::repo::channels`, with the viewer's library filters
//! applied the way the Windows Live screen applies them, so the two show the same list.

use aurora_db::repo::channels::{self, ChannelFilter};
use aurora_db::repo::filtering::LibraryFilter;

use crate::error::Result;
use crate::AuroraCore;

/// A channel as a list shows it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Channel {
    pub id: i64,
    pub name: String,
    pub number: Option<u32>,
    pub logo: Option<String>,
    pub group: Option<String>,
    pub has_catchup: bool,
    /// The provider stopped listing it at the last refresh. Kept, flagged.
    pub stale: bool,
    /// In the profile's Favourites.
    pub favorite: bool,
}

impl From<channels::ChannelRow> for Channel {
    fn from(c: channels::ChannelRow) -> Self {
        Self {
            id: c.id,
            name: c.name,
            number: c.number,
            logo: c.logo,
            group: c.group,
            has_catchup: c.has_catchup,
            stale: c.stale,
            favorite: c.favorite,
        }
    }
}

/// A group in the sidebar, with how many channels it holds.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ChannelGroup {
    pub name: String,
    pub count: i64,
}

#[uniffi::export]
impl AuroraCore {
    /// One page of channels, optionally in one group or only the profile's Favourites.
    ///
    /// Paged because a panel can list 22,000 channels and a tvOS `List` wants them as it
    /// scrolls, not all at once.
    pub fn channels(
        &self,
        profile_id: i64,
        group: Option<String>,
        favorites_only: bool,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Channel>> {
        let db = self.db.lock();
        let filter = ChannelFilter {
            group,
            limit: Some(limit),
            offset,
            library: LibraryFilter::load(&db)?,
            profile_id: Some(profile_id),
            favorites_only,
            ..Default::default()
        };
        Ok(channels::list(&db, &filter)?
            .into_iter()
            .map(Channel::from)
            .collect())
    }

    /// Add a channel to the profile's Favourites, or take it out. Returns whether it is
    /// a favourite now.
    pub fn toggle_favorite(&self, profile_id: i64, channel_id: i64) -> Result<bool> {
        let db = self.db.lock();
        Ok(aurora_db::repo::lists::toggle_favorite(
            &db,
            profile_id,
            channel_id,
            None,
            crate::now_unix(),
        )?)
    }

    /// Every group that has a visible channel, alphabetically.
    pub fn channel_groups(&self) -> Result<Vec<ChannelGroup>> {
        let db = self.db.lock();
        let filter = LibraryFilter::load(&db)?;
        Ok(channels::groups(&db, &filter)?
            .into_iter()
            .map(|(name, count)| ChannelGroup { name, count })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::core;

    #[test]
    fn an_empty_library_lists_nothing_rather_than_failing() {
        let (core, _dir) = core();
        assert!(core.channels(1, None, false, 50, 0).unwrap().is_empty());
        assert!(core.channel_groups().unwrap().is_empty());
    }
}
