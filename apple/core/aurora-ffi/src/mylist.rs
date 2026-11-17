//! My List and likes: films and series a profile saved or liked. Straight calls into
//! `aurora_db::repo::lists`, as Windows' `mylist.toggle`, `likes.toggle` and
//! `lists.marks` are.

use aurora_db::repo::{library, lists};

use crate::error::Result;
use crate::vod::{Movie, Series};
use crate::{now_unix, AuroraCore};

/// What My List and likes hold: films and series, not episodes or channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ListKind {
    Movie,
    Series,
}

impl From<ListKind> for lists::ItemKind {
    fn from(k: ListKind) -> Self {
        match k {
            ListKind::Movie => lists::ItemKind::Movie,
            ListKind::Series => lists::ItemKind::Series,
        }
    }
}

/// One profile's My List, newest first, as the library rows a poster grid draws.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MyList {
    pub movies: Vec<Movie>,
    pub series: Vec<Series>,
}

/// Whether one film or series is on My List and liked, for its detail screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ItemMarks {
    pub in_my_list: bool,
    pub liked: bool,
}

#[uniffi::export]
impl AuroraCore {
    /// Add to My List, or take off. Returns whether it is on the list now.
    pub fn toggle_my_list(&self, profile_id: i64, kind: ListKind, id: i64) -> Result<bool> {
        let db = self.db.lock();
        Ok(lists::toggle_my_list(
            &db,
            profile_id,
            kind.into(),
            id,
            now_unix(),
        )?)
    }

    /// Like, or unlike. Returns whether it is liked now.
    pub fn toggle_liked(&self, profile_id: i64, kind: ListKind, id: i64) -> Result<bool> {
        let db = self.db.lock();
        Ok(lists::toggle_liked(
            &db,
            profile_id,
            kind.into(),
            id,
            now_unix(),
        )?)
    }

    pub fn item_marks(&self, profile_id: i64, kind: ListKind, id: i64) -> Result<ItemMarks> {
        let db = self.db.lock();
        let kind: lists::ItemKind = kind.into();
        Ok(ItemMarks {
            in_my_list: lists::in_my_list(&db, profile_id, kind, id)?,
            liked: lists::liked(&db, profile_id)?.contains(&(kind, id)),
        })
    }

    /// The list itself. Anything a refresh removed from the library is left out rather
    /// than shown as a blank poster.
    pub fn my_list(&self, profile_id: i64) -> Result<MyList> {
        let db = self.db.lock();
        let mut out = MyList {
            movies: Vec::new(),
            series: Vec::new(),
        };
        for (kind, id) in lists::my_list(&db, profile_id)? {
            match kind {
                lists::ItemKind::Movie => {
                    if let Some(m) = library::movie(&db, id)? {
                        out.movies.push(m.into());
                    }
                }
                lists::ItemKind::Series => {
                    if let Some(s) = library::series(&db, id)? {
                        out.series.push(s.into());
                    }
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::ListKind;
    use crate::testing::core;

    #[test]
    fn an_empty_my_list_is_empty_and_marks_default_off() {
        let (core, _dir) = core();
        let list = core.my_list(1).unwrap();
        assert!(list.movies.is_empty() && list.series.is_empty());
        let marks = core.item_marks(1, ListKind::Movie, 42).unwrap();
        assert!(!marks.in_my_list && !marks.liked);
    }
}
