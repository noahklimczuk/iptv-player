//! Films and series: browsing them, playing them, and remembering where the viewer got to.
//!
//! Browsing and progress are straight calls into `aurora_db::repo::{library, progress}`,
//! so the rules that matter — what counts as finished, that "finished" is sticky, which
//! episode is next — are the shared ones. Tuning mirrors the `"movie"` and `"episode"`
//! arms of `aurora-app/src/play/resolve.rs` `resolve`.

use std::collections::HashMap;

use aurora_core::model::MediaKind;
use aurora_db::repo::filtering::{Kind, LibraryFilter};
use aurora_db::repo::library::{self, BrowseQuery, MovieSort};
use aurora_db::repo::progress::{self, ItemKind};
use aurora_db::rusqlite::OptionalExtension;
use aurora_video::Request;

use crate::error::{AuroraError, Result};
use crate::playback::{mpv_options, stream_url, MpvOption, StreamRef};
use crate::{now_unix, AuroraCore};

/// Network buffer for films and episodes, in seconds (README §6.1).
///
/// Mirrors `aurora-app/src/play/resolve.rs` `FILM_BUFFER_SECS`; `mirrors.rs` fails if
/// the two disagree.
pub(crate) const VOD_CACHE_SECS: u32 = 30;

/// How a browse page is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseSort {
    RecentlyAdded,
    Title,
    Year,
    Rating,
}

impl From<BrowseSort> for MovieSort {
    fn from(s: BrowseSort) -> Self {
        match s {
            BrowseSort::RecentlyAdded => MovieSort::RecentlyAdded,
            BrowseSort::Title => MovieSort::Title,
            BrowseSort::Year => MovieSort::Year,
            BrowseSort::Rating => MovieSort::Rating,
        }
    }
}

/// One page of a film or series shelf.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Browse {
    pub sort: BrowseSort,
    /// The provider's own shelf (its category), or every shelf.
    pub category: Option<String>,
    /// One genre, as [`BrowseFacets::genres`] names it, or every genre.
    pub genre: Option<String>,
    /// Narrow by title.
    pub query: Option<String>,
    /// Titles starting with this letter, or `#` for those starting with anything else.
    /// A filter rather than a scroll position, because the list is paged.
    pub letter: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

/// What a browse screen can narrow by, and how much the current filters match: Windows'
/// `library_browse_facets`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BrowseFacets {
    /// The provider's own shelves, biggest first.
    pub categories: Vec<Shelf>,
    /// Genres for this kind only (films and series don't share a list: offering the
    /// other's is a filter that matches nothing), commonest first.
    pub genres: Vec<Shelf>,
    /// Everything these filters match, not just the page on screen.
    pub total: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Movie {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub runtime_mins: Option<u32>,
    pub rating: Option<f32>,
    pub genres: Vec<String>,
}

impl From<library::MovieRow> for Movie {
    fn from(m: library::MovieRow) -> Self {
        Self {
            id: m.id,
            title: m.title,
            year: m.year,
            poster: m.poster,
            backdrop: m.backdrop,
            overview: m.overview,
            runtime_mins: m.runtime_mins,
            rating: m.rating,
            genres: m.genres,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Series {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub rating: Option<f32>,
    pub genres: Vec<String>,
    /// Season numbers that have episodes. Empty until the episode listing has been
    /// fetched for this show.
    pub seasons: Vec<u16>,
}

impl From<library::SeriesRow> for Series {
    fn from(s: library::SeriesRow) -> Self {
        Self {
            id: s.id,
            title: s.title,
            year: s.year,
            poster: s.poster,
            backdrop: s.backdrop,
            overview: s.overview,
            rating: s.rating,
            genres: s.genres,
            seasons: s.seasons,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Episode {
    pub id: i64,
    pub series_id: i64,
    pub season: u16,
    pub episode: u16,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub still: Option<String>,
    pub runtime_mins: Option<u32>,
}

impl From<library::EpisodeRow> for Episode {
    fn from(e: library::EpisodeRow) -> Self {
        Self {
            id: e.id,
            series_id: e.series_id,
            season: e.season,
            episode: e.episode,
            title: e.title,
            overview: e.overview,
            still: e.still,
            runtime_mins: e.runtime_mins,
        }
    }
}

/// A provider shelf and how much is on it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Shelf {
    pub name: String,
    pub count: u32,
}

/// What can be watched with a position: a film or an episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VodKind {
    Movie,
    Episode,
}

impl From<VodKind> for ItemKind {
    fn from(k: VodKind) -> Self {
        match k {
            VodKind::Movie => ItemKind::Movie,
            VodKind::Episode => ItemKind::Episode,
        }
    }
}

/// Where the viewer got to in a film or episode.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WatchProgress {
    pub position_secs: i64,
    pub duration_secs: i64,
    pub completed: bool,
    pub updated_at: i64,
}

/// Everything needed to start a film or episode.
#[derive(Debug, Clone, uniffi::Record)]
pub struct VodTune {
    pub kind: VodKind,
    pub id: i64,
    pub title: String,
    /// Can carry the provider's credentials: hand it to the player, never log it.
    pub url: String,
    /// Set on mpv before `loadfile`. Includes the start position, when one was asked for.
    pub mpv_options: Vec<MpvOption>,
}

/// The episode a series card should open on, and where in it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SeriesResume {
    pub episode_id: i64,
    pub position_secs: i64,
}

#[uniffi::export]
impl AuroraCore {
    pub fn movies(&self, browse: Browse) -> Result<Vec<Movie>> {
        let db = self.db.lock();
        let q = browse_query(&db, browse)?;
        Ok(library::list_movies(&db, &q)?
            .into_iter()
            .map(Movie::from)
            .collect())
    }

    pub fn movie_shelves(&self) -> Result<Vec<Shelf>> {
        self.shelves(Kind::Movies)
    }

    pub fn series_list(&self, browse: Browse) -> Result<Vec<Series>> {
        let db = self.db.lock();
        let q = browse_query(&db, browse)?;
        Ok(library::list_series(&db, &q)?
            .into_iter()
            .map(Series::from)
            .collect())
    }

    pub fn series_shelves(&self) -> Result<Vec<Shelf>> {
        self.shelves(Kind::Series)
    }

    /// The shelves and genres a film screen can narrow by, and how many films `browse`
    /// matches (its sort, limit and offset are ignored).
    pub fn movie_facets(&self, browse: Browse) -> Result<BrowseFacets> {
        self.facets(Kind::Movies, browse)
    }

    /// As [`Self::movie_facets`], for series.
    pub fn series_facets(&self, browse: Browse) -> Result<BrowseFacets> {
        self.facets(Kind::Series, browse)
    }

    pub fn series_detail(&self, series_id: i64) -> Result<Series> {
        let db = self.db.lock();
        library::series(&db, series_id)?
            .map(Series::from)
            .ok_or_else(|| AuroraError::not_found("This series is no longer in your library."))
    }

    /// One season's episodes, or every episode when `season` is empty.
    pub fn episodes(&self, series_id: i64, season: Option<u16>) -> Result<Vec<Episode>> {
        let db = self.db.lock();
        Ok(library::episodes_for(&db, series_id, season)?
            .into_iter()
            .map(Episode::from)
            .collect())
    }

    /// Everything needed to play a film, starting at `start_at_secs` if given.
    pub fn tune_movie(&self, movie_id: i64, start_at_secs: Option<f64>) -> Result<VodTune> {
        self.tune_vod(VodKind::Movie, movie_id, start_at_secs)
    }

    /// Everything needed to play an episode, starting at `start_at_secs` if given.
    pub fn tune_episode(&self, episode_id: i64, start_at_secs: Option<f64>) -> Result<VodTune> {
        self.tune_vod(VodKind::Episode, episode_id, start_at_secs)
    }

    pub fn watch_progress(
        &self,
        profile_id: i64,
        kind: VodKind,
        id: i64,
    ) -> Result<Option<WatchProgress>> {
        let db = self.db.lock();
        Ok(
            progress::get(&db, profile_id, kind.into(), id)?.map(|p| WatchProgress {
                position_secs: p.position_secs,
                duration_secs: p.duration_secs,
                completed: p.completed,
                updated_at: p.updated_at,
            }),
        )
    }

    /// Record a position. Returns whether this counts as finished — decided by the
    /// shared rule (`aurora_db::repo::progress::save`), never by the caller.
    pub fn save_progress(
        &self,
        profile_id: i64,
        kind: VodKind,
        id: i64,
        position_secs: i64,
        duration_secs: i64,
    ) -> Result<bool> {
        let db = self.db.lock();
        Ok(progress::save(
            &db,
            profile_id,
            kind.into(),
            id,
            position_secs,
            duration_secs,
            now_unix(),
        )?)
    }

    pub fn mark_watched(
        &self,
        profile_id: i64,
        kind: VodKind,
        id: i64,
        watched: bool,
    ) -> Result<()> {
        let db = self.db.lock();
        Ok(progress::mark_watched(
            &db,
            profile_id,
            kind.into(),
            id,
            watched,
            now_unix(),
        )?)
    }

    /// Which episode a series opens on: the one left part-way, else the next unwatched.
    pub fn series_resume(&self, profile_id: i64, series_id: i64) -> Result<Option<SeriesResume>> {
        let db = self.db.lock();
        Ok(progress::resume_point(&db, profile_id, series_id)?.map(
            |(episode_id, position_secs)| SeriesResume {
                episode_id,
                position_secs,
            },
        ))
    }
}

impl AuroraCore {
    /// Mirrors `aurora-app/src/commands.rs` `library_browse_facets`.
    fn facets(&self, kind: Kind, browse: Browse) -> Result<BrowseFacets> {
        let db = self.db.lock();
        // The letter counts too: the total is of what is on screen.
        let q = browse_query(&db, Browse { limit: 1, offset: 0, ..browse })?;
        let shelf = |c: library::Category| Shelf {
            name: c.name,
            count: c.count,
        };
        Ok(BrowseFacets {
            categories: library::categories(&db, kind, &q.library)?
                .into_iter()
                .map(shelf)
                .collect(),
            genres: library::genres(&db, kind, &q.library)?
                .into_iter()
                .map(shelf)
                .collect(),
            total: match kind {
                Kind::Series => library::count_series(&db, &q)?,
                _ => library::count_movies(&db, &q)?,
            },
        })
    }

    fn shelves(&self, kind: Kind) -> Result<Vec<Shelf>> {
        let db = self.db.lock();
        let filter = LibraryFilter::load(&db)?;
        Ok(library::categories(&db, kind, &filter)?
            .into_iter()
            .map(|c| Shelf {
                name: c.name,
                count: c.count,
            })
            .collect())
    }

    /// Mirrors the `What::Movie` and `What::Episode` arms of `aurora-app/src/play/resolve.rs`
    /// `resolve`, including the title shown (`shown_title`, R-76).
    fn tune_vod(&self, kind: VodKind, id: i64, start_at_secs: Option<f64>) -> Result<VodTune> {
        let db = self.db.lock();
        let (sql, noun, media) = match kind {
            VodKind::Movie => (
                "SELECT url, shown_title(COALESCE(custom_title, tmdb_title, title)),
                        stream_id, container, provider_id
                 FROM movies WHERE id = ?1",
                "film",
                MediaKind::Movie,
            ),
            VodKind::Episode => (
                "SELECT e.url, shown_title(e.title), e.stream_id, e.container, s.provider_id
                 FROM episodes e JOIN series s ON s.id = e.series_id
                 WHERE e.id = ?1",
                "episode",
                MediaKind::Episode,
            ),
        };
        let (stream, title) = db
            .query_row(sql, [id], |r| {
                Ok((
                    StreamRef {
                        url: r.get(0)?,
                        stream_id: r.get(2)?,
                        container: r.get(3)?,
                        provider_id: r.get(4)?,
                    },
                    r.get::<_, Option<String>>(1)?,
                ))
            })
            .optional()?
            .ok_or_else(|| {
                AuroraError::not_found(format!(
                    "This {noun} is no longer in your library. A refresh may have removed it."
                ))
            })?;

        let url = stream_url(
            &db,
            self.credentials.as_ref(),
            &mut HashMap::new(),
            media,
            &stream,
        )?;
        if url.trim().is_empty() {
            return Err(AuroraError::other(format!(
                "Your provider listed this {noun} without a playable address, so there is \
                 nothing to open. A library refresh may fill it in."
            )));
        }
        let title = title.unwrap_or_else(|| noun.to_string());

        let options = Request {
            live: false,
            cache_secs: VOD_CACHE_SECS,
            start_secs: start_at_secs,
            ..Default::default()
        };
        Ok(VodTune {
            kind,
            id,
            title,
            url,
            mpv_options: mpv_options(&options),
        })
    }
}

fn browse_query(db: &aurora_db::rusqlite::Connection, b: Browse) -> Result<BrowseQuery> {
    Ok(BrowseQuery {
        sort: b.sort.into(),
        category: b.category,
        genre: b.genre.filter(|g| !g.trim().is_empty()),
        query: b.query.filter(|q| !q.trim().is_empty()),
        letter: b.letter.as_deref().and_then(browse_letter),
        limit: b.limit.clamp(1, 500),
        offset: b.offset,
        library: LibraryFilter::load(db)?,
        ..Default::default()
    })
}

/// One letter, upper-cased, or `#`; anything else is no filter. Mirrors Windows'
/// `browse_query`: this reaches a SQL comparison against a title's first letter, so "Th"
/// would match nothing and look broken.
fn browse_letter(raw: &str) -> Option<String> {
    let mut chars = raw.trim().chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if first == '#' {
        return Some("#".into());
    }
    first.is_alphabetic().then(|| first.to_uppercase().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::core;

    fn browse() -> Browse {
        Browse {
            sort: BrowseSort::RecentlyAdded,
            category: None,
            genre: None,
            query: None,
            letter: None,
            limit: 50,
            offset: 0,
        }
    }

    /// Films straight into the table: (title, shelf, genres as JSON).
    fn films(core: &AuroraCore, rows: &[(&str, &str, &str)]) {
        let db = core.db.lock();
        db.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at) VALUES (1, 'P', 'm3u', 'http://p', 0)",
            [],
        )
        .unwrap();
        for (i, (title, shelf, genres)) in rows.iter().enumerate() {
            db.execute(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, group_title,
                                     url, genres, added_at, last_seen_at)
                 VALUES (1, ?1, ?2, lower(?2), ?3, 'http://p/m.mp4', ?4, ?5, 0)",
                aurora_db::rusqlite::params![format!("k{i}"), title, shelf, genres, i as i64],
            )
            .unwrap();
        }
    }

    #[test]
    fn a_letter_is_one_letter_or_hash() {
        assert_eq!(browse_letter("s").as_deref(), Some("S"));
        assert_eq!(browse_letter(" # ").as_deref(), Some("#"));
        assert_eq!(browse_letter("Th"), None);
        assert_eq!(browse_letter("7"), None);
        assert_eq!(browse_letter(""), None);
    }

    #[test]
    fn genre_and_letter_narrow_the_list_and_the_total_agrees() {
        let (core, _dir) = core();
        films(
            &core,
            &[
                ("Alien", "Sci-Fi Films", r#"["Science Fiction","Horror"]"#),
                ("Arrival", "Sci-Fi Films", r#"["Science Fiction","Drama"]"#),
                ("Brooklyn", "Dramas", r#"["Drama"]"#),
                ("9 to 5", "Comedies", r#"["Comedy"]"#),
            ],
        );
        let titles = |b: Browse| -> Vec<String> {
            let mut t: Vec<_> = core.movies(b).unwrap().into_iter().map(|m| m.title).collect();
            t.sort();
            t
        };

        let drama = Browse { genre: Some("Drama".into()), ..browse() };
        assert_eq!(titles(drama.clone()), ["Arrival", "Brooklyn"]);
        assert_eq!(core.movie_facets(drama).unwrap().total, 2);

        let a = Browse { letter: Some("a".into()), ..browse() };
        assert_eq!(titles(a.clone()), ["Alien", "Arrival"]);
        assert_eq!(core.movie_facets(a).unwrap().total, 2);

        let hash = Browse { letter: Some("#".into()), ..browse() };
        assert_eq!(titles(hash), ["9 to 5"]);

        let facets = core.movie_facets(browse()).unwrap();
        assert_eq!(facets.total, 4);
        assert_eq!(facets.categories.len(), 3);
        let drama = facets.genres.iter().find(|g| g.name == "Drama").unwrap();
        assert_eq!(drama.count, 2);
        // Films' genres only: there are no series, so no series genre can be offered.
        assert!(core.series_facets(browse()).unwrap().genres.is_empty());
    }

    #[test]
    fn an_empty_library_browses_to_nothing() {
        let (core, _dir) = core();
        assert!(core.movies(browse()).unwrap().is_empty());
        assert!(core.series_list(browse()).unwrap().is_empty());
        assert!(core.movie_shelves().unwrap().is_empty());
    }

    #[test]
    fn a_film_that_is_gone_says_so_in_words() {
        let (core, _dir) = core();
        let e = core.tune_movie(1, None).unwrap_err();
        assert!(e.to_string().contains("no longer in your library"), "{e}");
    }

    /// Progress goes through the shared rule, so "finished" means what it means on
    /// Windows: 92% of a film.
    #[test]
    fn progress_is_saved_and_finished_by_the_shared_rule() {
        let (core, _dir) = core();
        // Profile 1 is the one every library starts with (schema migration).
        assert!(!core.save_progress(1, VodKind::Movie, 5, 600, 6000).unwrap());
        let p = core.watch_progress(1, VodKind::Movie, 5).unwrap().unwrap();
        assert_eq!(p.position_secs, 600);
        assert!(!p.completed);
        assert!(core
            .save_progress(1, VodKind::Movie, 5, 5900, 6000)
            .unwrap());
    }
}
