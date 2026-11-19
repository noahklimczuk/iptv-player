//! One search box across the whole library: channels, what is on now and later, films,
//! series, and the cast and crew of what is in it. `aurora_db::repo::search::grouped`, which also applies the viewer's
//! library filters, so search never finds what they chose to hide.

use aurora_db::repo::search;

use crate::error::Result;
use crate::{now_unix, AuroraCore};

/// One result. What `id` names depends on the list it is in: a channel for `channels`,
/// `on_now` and `upcoming` (so a programme opens its channel), a film for `movies`, a
/// show for `series`, a TMDB person for `people`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchHit {
    pub id: i64,
    pub title: String,
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct SearchResults {
    pub channels: Vec<SearchHit>,
    pub on_now: Vec<SearchHit>,
    pub upcoming: Vec<SearchHit>,
    pub movies: Vec<SearchHit>,
    pub series: Vec<SearchHit>,
    /// Only people credited on something in the library.
    pub people: Vec<SearchHit>,
}

fn hits(from: Vec<search::SearchHit>) -> Vec<SearchHit> {
    from.into_iter()
        .map(|h| SearchHit {
            id: h.ref_id,
            title: h.title,
            subtitle: h.subtitle,
        })
        .collect()
}

#[uniffi::export]
impl AuroraCore {
    /// Up to `limit` results per list. Blank text finds nothing.
    pub fn search(&self, text: String, limit: u32) -> Result<SearchResults> {
        if text.trim().is_empty() {
            return Ok(SearchResults::default());
        }
        let db = self.db.lock();
        let r = search::grouped(&db, &text, now_unix(), limit.clamp(1, 100))?;
        Ok(SearchResults {
            channels: hits(r.channels),
            on_now: hits(r.on_now),
            upcoming: hits(r.upcoming),
            movies: hits(r.movies),
            series: hits(r.series),
            people: hits(r.people),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::core;

    #[test]
    fn blank_text_finds_nothing_without_asking_the_index() {
        let (core, _dir) = core();
        assert_eq!(core.search("   ".into(), 20).unwrap(), Default::default());
    }

    #[test]
    fn an_empty_library_finds_nothing() {
        let (core, _dir) = core();
        let r = core.search("news".into(), 20).unwrap();
        assert!(r.channels.is_empty() && r.movies.is_empty() && r.people.is_empty());
    }
}
