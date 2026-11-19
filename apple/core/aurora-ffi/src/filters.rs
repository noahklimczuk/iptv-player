//! The library-wide filters: hide other languages, hide what is untagged, show one copy
//! of each title.
//!
//! The rules are the shared `aurora_db::repo::filtering` ones, and every list in this
//! crate already applies the saved setting (`LibraryFilter::load`), channels included.
//! This module only reads and saves it, and counts what each switch hides, as Windows'
//! settings panel does (`aurora-app/src/playlist.rs` `library_filters`,
//! `library_set_filters`, `library_filter_counts`).

use aurora_db::repo::filtering::{self, Kind, LibraryFilter};

use crate::error::Result;
use crate::AuroraCore;

/// The three switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LibraryFilters {
    /// Hide what the provider tagged as another language. Untagged entries stay.
    pub english_only: bool,
    /// Also hide what has no language tag at all. Only meaningful with `english_only`:
    /// on a playlist that tags nothing, it hides everything, which the counts show.
    pub hide_untagged: bool,
    /// One entry per title, the best copy of it.
    pub hide_duplicates: bool,
}

impl From<LibraryFilter> for LibraryFilters {
    fn from(f: LibraryFilter) -> Self {
        Self {
            english_only: f.english_only,
            hide_untagged: f.hide_untagged,
            hide_duplicates: f.hide_duplicates,
        }
    }
}

impl From<LibraryFilters> for LibraryFilter {
    fn from(f: LibraryFilters) -> Self {
        Self {
            english_only: f.english_only,
            hide_untagged: f.hide_untagged,
            hide_duplicates: f.hide_duplicates,
        }
    }
}

/// Which list a count is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LibraryKind {
    Channels,
    Movies,
    Series,
}

impl From<LibraryKind> for Kind {
    fn from(k: LibraryKind) -> Self {
        match k {
            LibraryKind::Channels => Kind::Live,
            LibraryKind::Movies => Kind::Movies,
            LibraryKind::Series => Kind::Series,
        }
    }
}

/// What one list holds and what each filter takes out of it: the numbers a viewer needs
/// before turning a switch on, since on a big playlist "English only" is either a relief
/// or empties the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct FilterCounts {
    /// Everything in the list, before any of these filters.
    pub total: i64,
    /// Tagged as another language: what "English only" hides.
    pub non_english: i64,
    /// With no language tag: what "hide untagged" adds.
    pub untagged: i64,
    /// Lesser copies of a title: what "one copy per title" hides.
    pub duplicates: i64,
}

impl From<filtering::FilterCounts> for FilterCounts {
    fn from(c: filtering::FilterCounts) -> Self {
        Self {
            total: c.total,
            non_english: c.non_english,
            untagged: c.untagged,
            duplicates: c.duplicates,
        }
    }
}

#[uniffi::export]
impl AuroraCore {
    pub fn library_filters(&self) -> Result<LibraryFilters> {
        let db = self.db.lock();
        Ok(LibraryFilter::load(&db)?.into())
    }

    /// Save the switches and return them as saved. Every list reads them, so the app
    /// reloads what it shows afterwards.
    pub fn set_library_filters(&self, filters: LibraryFilters) -> Result<LibraryFilters> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let db = self.db.lock();
        LibraryFilter::from(filters).save(&db)?;
        Ok(LibraryFilter::load(&db)?.into())
    }

    pub fn filter_counts(&self, kind: LibraryKind) -> Result<FilterCounts> {
        let db = self.db.lock();
        Ok(filtering::counts(&db, kind.into())?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::core;

    #[test]
    fn the_switches_are_saved_and_read_back() {
        let (core, _dir) = core();
        let on = LibraryFilters {
            english_only: true,
            hide_untagged: true,
            hide_duplicates: false,
        };
        assert_eq!(core.set_library_filters(on).unwrap(), on);
        assert_eq!(core.library_filters().unwrap(), on);
    }

    #[test]
    fn an_empty_library_counts_nothing() {
        let (core, _dir) = core();
        for kind in [
            LibraryKind::Channels,
            LibraryKind::Movies,
            LibraryKind::Series,
        ] {
            assert_eq!(core.filter_counts(kind).unwrap().total, 0);
        }
    }

    /// The switch reaches the lists: a channel tagged as another language goes when
    /// "English only" is on, and an untagged one stays until "hide untagged" is too.
    #[test]
    fn english_only_hides_tagged_channels_and_keeps_untagged_ones() {
        let (core, _dir) = core();
        {
            let db = core.db.lock();
            db.execute(
                "INSERT INTO providers (id, name, kind, base_url, created_at) VALUES (1, 'P', 'm3u', 'http://p', 0)",
                [],
            )
            .unwrap();
            for (id, name, lang) in [
                (1, "BBC One", Some("en")),
                (2, "TVE", Some("es")),
                (3, "Mystery", None),
            ] {
                db.execute(
                    "INSERT INTO channels (id, provider_id, provider_key, name, match_key, lang_code, last_seen_at)
                     VALUES (?1, 1, ?2, ?2, lower(?2), ?3, 0)",
                    aurora_db::rusqlite::params![id, name, lang],
                )
                .unwrap();
            }
        }
        let names = || -> Vec<String> {
            let mut n: Vec<_> = core
                .channels(1, None, false, None, 100, 0)
                .unwrap()
                .into_iter()
                .map(|c| c.name)
                .collect();
            n.sort();
            n
        };
        assert_eq!(names().len(), 3);

        let mut f = core.library_filters().unwrap();
        f.english_only = true;
        core.set_library_filters(f).unwrap();
        assert_eq!(names(), ["BBC One", "Mystery"]);
        let counts = core.filter_counts(LibraryKind::Channels).unwrap();
        assert_eq!(
            (counts.total, counts.non_english, counts.untagged),
            (3, 1, 1)
        );

        f.hide_untagged = true;
        core.set_library_filters(f).unwrap();
        assert_eq!(names(), ["BBC One"]);
    }
}
