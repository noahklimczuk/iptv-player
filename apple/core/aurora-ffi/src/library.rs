//! Reading the library: channels and their groups.
//!
//! Straight calls into `aurora_db::repo::channels`, with the viewer's library filters
//! applied the way the Windows Live screen applies them, so the two show the same list.

use aurora_db::repo::channels::{self, ChannelFilter};
use aurora_db::repo::filtering::LibraryFilter;
use aurora_db::repo::lists;

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
    /// In the list asked about: the profile's Favourites unless another was named.
    pub favorite: bool,
}

/// One of a profile's channel lists.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FavoriteList {
    pub name: String,
    pub channels: u32,
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
        favorites_list: Option<String>,
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
            // Which named list `favorite` answers for, and `favorites_only` shows. The
            // default list when absent, as on Windows.
            favorites_list,
            ..Default::default()
        };
        Ok(channels::list(&db, &filter)?
            .into_iter()
            .map(Channel::from)
            .collect())
    }

    /// Add a channel to one of the profile's lists, or take it out: `list` names it, the
    /// default Favourites when absent. A name nobody has used yet makes a new list, as on
    /// Windows, where a list exists because something is in it. Returns whether the
    /// channel is in that list now.
    pub fn toggle_favorite(
        &self,
        profile_id: i64,
        channel_id: i64,
        list: Option<String>,
    ) -> Result<bool> {
        let db = self.db.lock();
        Ok(lists::toggle_favorite(
            &db,
            profile_id,
            channel_id,
            list.as_deref(),
            crate::now_unix(),
        )?)
    }

    /// The profile's lists, Favourites first (always there, even empty), then the rest
    /// alphabetically, each with how many channels it holds.
    pub fn favorite_lists(&self, profile_id: i64) -> Result<Vec<FavoriteList>> {
        let db = self.db.lock();
        Ok(lists::favorite_lists(&db, profile_id)?
            .into_iter()
            .map(|l| FavoriteList {
                name: l.name,
                channels: l.channels,
            })
            .collect())
    }

    /// Rename a list; onto an existing name, the two merge. Returns how many channels
    /// moved.
    pub fn rename_favorite_list(&self, profile_id: i64, from: String, to: String) -> Result<u32> {
        let db = self.db.lock();
        Ok(lists::rename_favorite_list(&db, profile_id, &from, &to)? as u32)
    }

    /// Empty a list, which is deleting it. Returns how many channels were in it.
    pub fn delete_favorite_list(&self, profile_id: i64, name: String) -> Result<u32> {
        let db = self.db.lock();
        Ok(lists::delete_favorite_list(&db, profile_id, &name)? as u32)
    }

    /// Put a list in the order on screen: the whole list, as Windows sends it, so the
    /// result is exactly what the viewer sees.
    pub fn reorder_favorites(
        &self,
        profile_id: i64,
        list: Option<String>,
        channel_ids: Vec<i64>,
    ) -> Result<u32> {
        let mut db = self.db.lock();
        Ok(lists::reorder_favorites(&mut db, profile_id, list.as_deref(), &channel_ids)? as u32)
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
        assert!(core.channels(1, None, false, None, 50, 0).unwrap().is_empty());
        assert!(core.channel_groups().unwrap().is_empty());
    }
}
