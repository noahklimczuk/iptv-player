//! The playlist editor: rename, renumber, regroup, hide, and put back (README §7.3).
//!
//! The same calls as Windows' editor (`aurora-app/src/playlist.rs`), straight into the
//! shared `aurora_db::repo::playlist`. It is the one list that shows hidden rows too:
//! hiding something has to be reversible, and a viewer who cannot find what they hid
//! has lost it.

use aurora_db::repo::filtering;
use aurora_db::repo::playlist::{self, Patch, Query, Show};

use crate::error::Result;
use crate::filters::LibraryKind;
use crate::AuroraCore;

/// Which rows the editor lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PlaylistShow {
    All,
    Visible,
    Hidden,
}

impl From<PlaylistShow> for Show {
    fn from(s: PlaylistShow) -> Self {
        match s {
            PlaylistShow::All => Show::All,
            PlaylistShow::Visible => Show::Visible,
            PlaylistShow::Hidden => Show::Hidden,
        }
    }
}

/// What the editor is looking at: one list, narrowed by search, group, shown or hidden,
/// and optionally only what has another copy.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaylistQuery {
    pub kind: LibraryKind,
    pub text: Option<String>,
    pub group: Option<String>,
    pub show: PlaylistShow,
    pub duplicates_only: bool,
}

impl PlaylistQuery {
    fn query(&self, limit: u32, offset: u32) -> Query {
        Query {
            kind: self.kind.into(),
            text: self.text.clone().filter(|t| !t.trim().is_empty()),
            group: self.group.clone().filter(|g| !g.is_empty()),
            show: self.show.into(),
            duplicates_only: self.duplicates_only,
            // Windows' own bounds (`ListArgs::query`).
            limit: limit.clamp(1, 1000),
            offset,
        }
    }
}

/// One entry as the editor shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PlaylistEntry {
    pub id: i64,
    /// What it is called now: the viewer's name if they gave one.
    pub name: String,
    /// What the provider calls it, shown when the two differ.
    pub provider_name: String,
    pub number: Option<u32>,
    pub group: Option<String>,
    pub quality: Option<String>,
    pub lang: Option<String>,
    /// Channels only: the viewer's logo if they set one, else the provider's.
    pub logo: Option<String>,
    pub hidden: bool,
    /// Something was changed by hand, so the editor offers to put it back.
    pub edited: bool,
    /// How many other copies of this title exist.
    pub duplicates: u32,
    pub provider: Option<String>,
}

impl From<playlist::PlaylistRow> for PlaylistEntry {
    fn from(r: playlist::PlaylistRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            provider_name: r.provider_name,
            number: r.number,
            group: r.group,
            quality: r.quality,
            lang: r.lang,
            logo: r.logo,
            hidden: r.hidden,
            edited: r.edited,
            duplicates: r.duplicates,
            provider: r.provider,
        }
    }
}

/// A page of the editor's list and how many match in all, so it can say "412 channels"
/// and page without asking twice.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaylistPage {
    pub rows: Vec<PlaylistEntry>,
    pub total: i64,
}

/// A group and how many entries it holds, hidden ones included.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaylistGroup {
    pub name: String,
    pub count: i64,
}

/// One edit. A field left `None` is left alone; an empty string, or a number of 0,
/// clears the override and the provider's own value shows through again.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct PlaylistPatch {
    pub name: Option<String>,
    pub number: Option<u32>,
    pub group: Option<String>,
    pub logo: Option<String>,
    pub hidden: Option<bool>,
}

impl From<PlaylistPatch> for Patch {
    fn from(p: PlaylistPatch) -> Self {
        Patch {
            name: p.name,
            number: p.number,
            group: p.group,
            logo: p.logo,
            hidden: p.hidden,
        }
    }
}

/// Another copy of the same title, from this provider or another.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Alternate {
    pub id: i64,
    pub name: String,
    pub quality: Option<String>,
    pub provider: Option<String>,
}

/// What a typed channel number asks for: `Some(0)` puts the provider's number back,
/// `Some(n)` renumbers, `None` is a typo and nothing should be sent.
///
/// Windows' `numberPatch` (`src-ui/src/features/playlist/number.ts`, R-124): an empty
/// box is a real request (clear), digits are a number, and anything else — `12abc`,
/// `0x10`, `1e3`, other scripts' digits — is refused rather than read as 0, which would
/// silently throw away a renumbering. Bounded by what the column holds.
#[uniffi::export]
pub fn channel_number_from_typed(input: String) -> Option<u32> {
    let text = input.trim();
    if text.is_empty() {
        return Some(0);
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

#[uniffi::export]
impl AuroraCore {
    /// A page of the editor's list, hidden rows included unless asked otherwise.
    pub fn playlist_page(
        &self,
        query: PlaylistQuery,
        limit: u32,
        offset: u32,
    ) -> Result<PlaylistPage> {
        let q = query.query(limit, offset);
        let db = self.db.lock();
        Ok(PlaylistPage {
            rows: playlist::list(&db, &q)?
                .into_iter()
                .map(Into::into)
                .collect(),
            total: playlist::count(&db, &q)?,
        })
    }

    /// Every group in one list, with how many entries each holds.
    pub fn playlist_groups(&self, kind: LibraryKind) -> Result<Vec<PlaylistGroup>> {
        let db = self.db.lock();
        Ok(playlist::groups(&db, kind.into())?
            .into_iter()
            .map(|(name, count)| PlaylistGroup { name, count })
            .collect())
    }

    /// Change one entry.
    pub fn playlist_update(&self, kind: LibraryKind, id: i64, patch: PlaylistPatch) -> Result<()> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let db = self.db.lock();
        playlist::update(&db, kind.into(), id, &patch.into())?;
        Ok(())
    }

    /// Hide or show these entries. Returns how many changed.
    pub fn playlist_set_hidden(
        &self,
        kind: LibraryKind,
        ids: Vec<i64>,
        hidden: bool,
    ) -> Result<u32> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let mut db = self.db.lock();
        Ok(playlist::set_hidden_many(&mut db, kind.into(), &ids, hidden)? as u32)
    }

    /// Hide or show everything the query matches, not just the page on screen.
    pub fn playlist_hide_matching(&self, query: PlaylistQuery, hidden: bool) -> Result<u32> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        // Paging doesn't apply: `hide_matching` acts on the whole match.
        let q = query.query(1, 0);
        let db = self.db.lock();
        Ok(playlist::hide_matching(&db, &q, hidden)? as u32)
    }

    /// Put these entries back to what the provider says. Returns how many changed.
    pub fn playlist_reset(&self, kind: LibraryKind, ids: Vec<i64>) -> Result<u32> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let mut db = self.db.lock();
        Ok(playlist::reset(&mut db, kind.into(), &ids)? as u32)
    }

    /// The other copies of a title, so the viewer can pick which one plays.
    pub fn alternates(&self, kind: LibraryKind, id: i64) -> Result<Vec<Alternate>> {
        let db = self.db.lock();
        Ok(filtering::alternates(&db, kind.into(), id)?
            .into_iter()
            .map(|a| Alternate {
                id: a.id,
                name: a.name,
                quality: a.quality,
                provider: a.provider,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::channel_number_from_typed as typed;

    #[test]
    fn a_typed_number_means_what_windows_takes_it_to_mean() {
        assert_eq!(typed("".into()), Some(0), "empty clears");
        assert_eq!(typed("  ".into()), Some(0));
        assert_eq!(typed("0".into()), Some(0), "0 clears too");
        assert_eq!(typed(" 42 ".into()), Some(42));
        assert_eq!(typed("12abc".into()), None, "a typo sends nothing");
        assert_eq!(typed("0x10".into()), None);
        assert_eq!(typed("1e3".into()), None);
        assert_eq!(typed("-5".into()), None);
        assert_eq!(typed("٣".into()), None, "other scripts' digits are refused");
        assert_eq!(
            typed("99999999999".into()),
            None,
            "more than the column holds"
        );
    }
}
