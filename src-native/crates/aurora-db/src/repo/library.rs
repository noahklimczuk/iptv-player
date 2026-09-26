//! Movies, series, and episodes.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieRow {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub runtime_mins: Option<u32>,
    pub rating: Option<f32>,
    pub certification: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub added_at: Option<i64>,
    pub logo_art: Option<String>,
    /// ISO 639-1, or nothing when the title never said (README §7.3).
    pub lang: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewMovie {
    pub provider_key: String,
    pub title: String,
    pub match_key: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub group: Option<String>,
    pub url: String,
    pub poster: Option<String>,
    pub added_at: Option<i64>,
    /// The provider's own score out of ten. Enrichment overwrites it when TMDB has
    /// an opinion; until then it is the only rating the library has.
    pub rating: Option<f32>,
}

pub fn upsert_movies(
    conn: &mut Connection,
    provider_id: i64,
    movies: &[NewMovie],
    now: i64,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        // Enrichment-owned columns (overview, backdrop, logo_art, tmdb_id, …) are not in the
        // UPDATE list: a playlist refresh must not wipe metadata fetched from TMDB.
        let mut stmt = tx.prepare(
            r#"INSERT INTO movies
               (provider_id, provider_key, title, match_key, year, quality, group_title,
                url, poster, added_at, rating, last_seen_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
               ON CONFLICT (provider_id, provider_key) DO UPDATE SET
                 title        = excluded.title,
                 match_key    = excluded.match_key,
                 year         = excluded.year,
                 quality      = excluded.quality,
                 group_title  = excluded.group_title,
                 url          = excluded.url,
                 poster       = COALESCE(excluded.poster, movies.poster),
                 -- The existing one wins: by the second refresh it may be TMDB's, and
                 -- a playlist must not undo enrichment. The panel only fills a gap.
                 rating       = COALESCE(movies.rating, excluded.rating),
                 last_seen_at = excluded.last_seen_at"#,
        )?;
        for m in movies {
            stmt.execute(params![
                provider_id,
                m.provider_key,
                m.title,
                m.match_key,
                m.year,
                m.quality,
                m.group,
                m.url,
                m.poster,
                m.added_at.unwrap_or(now),
                m.rating,
                now
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// The stream URL is deliberately absent. It carries the provider's username and
/// password in the path, and the renderer has no use for it: playback is resolved on
/// the host (README C10).
const MOVIE_SELECT: &str = "SELECT movies.id, COALESCE(custom_title, title), year, quality,
                                   poster, backdrop, overview, runtime_mins, rating,
                                   genres, certification, added_at, logo_art, lang_code,
                                   (SELECT group_concat(p.name, char(31))
                                      FROM credits c JOIN people p ON p.tmdb_id = c.person_id
                                     WHERE c.item_kind = 'movie' AND c.item_id = movies.id
                                       AND c.is_cast = 1
                                     ORDER BY c.ord)
                            FROM movies";

/// `group_concat` cannot be ordered portably, and a comma would split names that
/// contain one, so the separator is a unit separator no name has.
fn split_names(raw: Option<String>) -> Vec<String> {
    raw.map(|s| {
        s.split('\u{1f}')
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

fn map_movie(r: &rusqlite::Row<'_>) -> rusqlite::Result<MovieRow> {
    let genres: Option<String> = r.get(9)?;
    Ok(MovieRow {
        id: r.get(0)?,
        title: r.get(1)?,
        year: r.get::<_, Option<i64>>(2)?.map(|v| v as i32),
        quality: r.get(3)?,
        poster: r.get(4)?,
        backdrop: r.get(5)?,
        overview: r.get(6)?,
        runtime_mins: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        rating: r.get::<_, Option<f64>>(8)?.map(|v| v as f32),
        genres: genres
            .and_then(|g| serde_json::from_str(&g).ok())
            .unwrap_or_default(),
        certification: r.get(10)?,
        added_at: r.get(11)?,
        logo_art: r.get(12)?,
        lang: r.get(13)?,
        cast: split_names(r.get(14)?),
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovieSort {
    #[default]
    RecentlyAdded,
    Title,
    Year,
    Rating,
}

/// What a browse page asks for: a sort, a page, some optional filters, and whatever
/// the library-wide filters are set to (README §7.3, §8.5).
#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    pub sort: MovieSort,
    pub genre: Option<String>,
    /// The provider's own shelf — `group_title`. On a library without a TMDB key this
    /// is the only structure there is: 202 categories on the subscription this was
    /// built against, and not one genre.
    pub category: Option<String>,
    /// Narrow by title. Substring rather than the FTS index on purpose: this filters
    /// a list somebody is already looking at, where the ranked cross-library search
    /// behind Ctrl-K is a different question with a different answer.
    pub query: Option<String>,
    pub limit: u32,
    pub offset: u32,
    pub library: crate::repo::filtering::LibraryFilter,
}

/// The `WHERE` a browse query adds beyond `hidden = 0`, and the parameters it needs.
///
/// Built together because they have to agree: a clause with no parameter bound, or a
/// parameter with no clause, is a runtime error rather than a compile-time one.
fn browse_filters<'a>(
    table: &str,
    q: &'a BrowseQuery,
) -> (String, Vec<(&'static str, &'a dyn rusqlite::ToSql)>) {
    let mut sql = String::new();
    let mut params: Vec<(&'static str, &'a dyn rusqlite::ToSql)> = Vec::new();

    // Genres are stored as a JSON array, so matching one means matching its text. The
    // quotes make `"Action"` fail to match `"Action Comedy"`, which a bare LIKE would
    // not.
    if let Some(genre) = &q.genre {
        sql.push_str(&format!(
            " AND {table}.genres LIKE '%\"' || :genre || '\"%'"
        ));
        params.push((":genre", genre));
    }
    if let Some(category) = &q.category {
        sql.push_str(&format!(" AND {table}.group_title = :category"));
        params.push((":category", category));
    }
    if let Some(query) = &q.query {
        sql.push_str(&format!(
            " AND COALESCE({table}.custom_title, {table}.title) LIKE '%' || :query || '%'"
        ));
        params.push((":query", query));
    }
    (sql, params)
}

pub fn list_movies(conn: &Connection, q: &BrowseQuery) -> Result<Vec<MovieRow>> {
    let order = match q.sort {
        MovieSort::RecentlyAdded => "added_at DESC, movies.id DESC",
        MovieSort::Title => "title COLLATE NOCASE",
        MovieSort::Year => "year DESC NULLS LAST, title",
        MovieSort::Rating => "rating DESC NULLS LAST, title",
    };
    let (filters, mut params) = browse_filters("movies", q);
    let sql = format!(
        "{MOVIE_SELECT} WHERE movies.hidden = 0{}{filters} ORDER BY {order} \
         LIMIT :limit OFFSET :offset",
        q.library.where_sql(crate::repo::filtering::Kind::Movies),
    );
    params.push((":limit", &q.limit));
    params.push((":offset", &q.offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params.as_slice(), map_movie)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// How many rows a browse query matches, ignoring its page.
///
/// So the heading can say "117,508" rather than "120+". The count used to be whatever
/// had been fetched so far, which on the first page was the page size wearing a
/// library's clothes — and after that was a number that climbed while you scrolled.
pub fn count_movies(conn: &Connection, q: &BrowseQuery) -> Result<u32> {
    let (filters, params) = browse_filters("movies", q);
    let sql = format!(
        "SELECT count(*) FROM movies WHERE movies.hidden = 0{}{filters}",
        q.library.where_sql(crate::repo::filtering::Kind::Movies),
    );
    Ok(conn.query_row(&sql, params.as_slice(), |r| r.get(0))?)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesRow {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub logo_art: Option<String>,
    pub overview: Option<String>,
    pub rating: Option<f32>,
    pub certification: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub seasons: Vec<u16>,
    pub added_at: Option<i64>,
    pub lang: Option<String>,
}

const SERIES_SELECT: &str = "SELECT series.id, COALESCE(custom_title, title), year, quality,
                                    poster, backdrop, logo_art, overview, rating,
                                    certification, genres, added_at, lang_code,
                                    (SELECT group_concat(p.name, char(31))
                                       FROM credits c JOIN people p ON p.tmdb_id = c.person_id
                                      WHERE c.item_kind = 'series' AND c.item_id = series.id
                                        AND c.is_cast = 1
                                      ORDER BY c.ord),
                                    (SELECT group_concat(DISTINCT e.season)
                                       FROM episodes e WHERE e.series_id = series.id)
                             FROM series";

fn map_series(r: &rusqlite::Row<'_>) -> rusqlite::Result<SeriesRow> {
    let genres: Option<String> = r.get(10)?;
    let seasons: Option<String> = r.get(14)?;
    let mut seasons: Vec<u16> = seasons
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default();
    seasons.sort_unstable();
    Ok(SeriesRow {
        id: r.get(0)?,
        title: r.get(1)?,
        year: r.get::<_, Option<i64>>(2)?.map(|v| v as i32),
        quality: r.get(3)?,
        poster: r.get(4)?,
        backdrop: r.get(5)?,
        logo_art: r.get(6)?,
        overview: r.get(7)?,
        rating: r.get::<_, Option<f64>>(8)?.map(|v| v as f32),
        certification: r.get(9)?,
        genres: genres
            .and_then(|g| serde_json::from_str(&g).ok())
            .unwrap_or_default(),
        added_at: r.get(11)?,
        lang: r.get(12)?,
        cast: split_names(r.get(13)?),
        seasons,
    })
}

pub fn list_series(conn: &Connection, q: &BrowseQuery) -> Result<Vec<SeriesRow>> {
    let order = match q.sort {
        MovieSort::RecentlyAdded => "added_at DESC, series.id DESC",
        MovieSort::Title => "title COLLATE NOCASE",
        MovieSort::Year => "year DESC NULLS LAST, title",
        MovieSort::Rating => "rating DESC NULLS LAST, title",
    };
    let (filters, mut params) = browse_filters("series", q);
    let sql = format!(
        "{SERIES_SELECT} WHERE series.hidden = 0{}{filters} ORDER BY {order} \
         LIMIT :limit OFFSET :offset",
        q.library.where_sql(crate::repo::filtering::Kind::Series),
    );
    params.push((":limit", &q.limit));
    params.push((":offset", &q.offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params.as_slice(), map_series)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// How many series a browse query matches, ignoring its page.
pub fn count_series(conn: &Connection, q: &BrowseQuery) -> Result<u32> {
    let (filters, params) = browse_filters("series", q);
    let sql = format!(
        "SELECT count(*) FROM series WHERE series.hidden = 0{}{filters}",
        q.library.where_sql(crate::repo::filtering::Kind::Series),
    );
    Ok(conn.query_row(&sql, params.as_slice(), |r| r.get(0))?)
}

/// One shelf the provider files titles under, and how many are on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub name: String,
    pub count: u32,
}

/// The provider's own categories, biggest first.
///
/// The structure a real library actually has. Genres come from TMDB enrichment, which
/// needs an API key a viewer may never set — so on most libraries the genre filter is
/// an empty dropdown, while the panel has been filing everything under 202 named
/// shelves the whole time.
pub fn categories(conn: &Connection, kind: crate::repo::filtering::Kind) -> Result<Vec<Category>> {
    let table = match kind {
        crate::repo::filtering::Kind::Movies => "movies",
        crate::repo::filtering::Kind::Series => "series",
        _ => return Ok(Vec::new()),
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT group_title, count(*) FROM {table}
         WHERE hidden = 0 AND group_title IS NOT NULL AND group_title != ''
         GROUP BY group_title
         ORDER BY count(*) DESC, group_title"
    ))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Category {
                name: r.get(0)?,
                count: r.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One title by id, for a list of ids somebody curated rather than a browse page.
///
/// Deliberately not filtered by `hidden`: My List holds what a person put there, and
/// a title vanishing from it because a bulk edit hid it would look like data loss.
pub fn movie(conn: &Connection, id: i64) -> Result<Option<MovieRow>> {
    let sql = format!("{MOVIE_SELECT} WHERE movies.id = ?1");
    Ok(conn.query_row(&sql, params![id], map_movie).optional()?)
}

pub fn series(conn: &Connection, id: i64) -> Result<Option<SeriesRow>> {
    let sql = format!("{SERIES_SELECT} WHERE series.id = ?1");
    Ok(conn.query_row(&sql, params![id], map_series).optional()?)
}

/// What the library holds, for the Settings screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub channels: i64,
    pub movies: i64,
    pub series: i64,
    pub episodes: i64,
    pub programmes: i64,
    pub epg_coverage: EpgCoverage,
}

/// How much of the guide actually reaches the channels it is for.
///
/// The number that matters is not how many programmes were imported but how many
/// channels ended up with any — an EPG that parsed perfectly and matched nothing looks
/// identical to a missing one from the sofa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpgCoverage {
    pub total: i64,
    pub matched: i64,
    /// A few names, to make "what is missing" answerable without a query tool. Capped:
    /// a playlist with nine thousand unmatched channels should not ship all nine
    /// thousand to the renderer to be printed in one line.
    pub unmatched: Vec<String>,
}

const UNMATCHED_SAMPLE: usize = 12;

pub fn stats(conn: &Connection) -> Result<LibraryStats> {
    let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };

    let total = count("SELECT COUNT(*) FROM channels WHERE hidden = 0")?;
    let matched = count(
        "SELECT COUNT(*) FROM channels
         WHERE hidden = 0 AND epg_channel_id IS NOT NULL AND epg_channel_id <> ''",
    )?;

    let mut stmt = conn.prepare(
        "SELECT COALESCE(custom_name, name) FROM channels
         WHERE hidden = 0 AND (epg_channel_id IS NULL OR epg_channel_id = '')
         ORDER BY COALESCE(custom_number, number), id
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([UNMATCHED_SAMPLE as i64], |r| r.get(0))?;
    let unmatched = rows.collect::<std::result::Result<Vec<String>, _>>()?;

    Ok(LibraryStats {
        channels: total,
        movies: count("SELECT COUNT(*) FROM movies WHERE hidden = 0")?,
        series: count("SELECT COUNT(*) FROM series WHERE hidden = 0")?,
        episodes: count("SELECT COUNT(*) FROM episodes")?,
        programmes: count("SELECT COUNT(*) FROM epg_programmes")?,
        epg_coverage: EpgCoverage {
            total,
            matched,
            unmatched,
        },
    })
}

/// Every genre present in one half of the library, for that screen's browse filter.
///
/// Scoped by kind, like `categories`. It used to union both tables, which did not
/// matter while genres arrived only from TMDB and a library generally had none of
/// them — and became wrong the moment the provider's own genres were kept
/// (docs/DECISIONS.md D26): `get_vod_streams` sends no genre, so Movies would offer a
/// dropdown of 28,529 shows' genres and filtering by one would return nothing at all.
///
/// `DISTINCT` before the JSON is parsed, because thousands of rows share a handful of
/// genre strings and this runs again on every change to the filters.
pub fn genres(conn: &Connection, kind: crate::repo::filtering::Kind) -> Result<Vec<String>> {
    let table = match kind {
        crate::repo::filtering::Kind::Movies => "movies",
        crate::repo::filtering::Kind::Series => "series",
        _ => return Ok(Vec::new()),
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT genres FROM {table}
          WHERE hidden = 0 AND genres IS NOT NULL AND genres != '' AND genres != '[]'"
    ))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = std::collections::BTreeSet::new();
    for raw in rows {
        // A row whose genres are malformed loses its genres, not the whole filter.
        if let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) {
            out.extend(list.into_iter().filter(|g| !g.trim().is_empty()));
        }
    }
    Ok(out.into_iter().collect())
}

/// README §13: the same film from three providers should be one card, not three.
pub fn duplicate_groups(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT match_key, count(*) c FROM movies GROUP BY match_key, COALESCE(year,0)
         HAVING c > 1 ORDER BY c DESC",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Default)]
pub struct NewEpisode {
    pub season: u16,
    pub episode: u16,
    pub title: Option<String>,
    pub url: String,
    pub still: Option<String>,
}

/// A show as the provider describes it, before enrichment.
#[derive(Debug, Clone, Default)]
pub struct NewSeries<'a> {
    pub provider_key: &'a str,
    pub title: &'a str,
    pub match_key: &'a str,
    pub year: Option<i32>,
    pub poster: Option<&'a str>,
    /// The provider's own category, which is also where a language tag often hides.
    pub group: Option<&'a str>,
    /// The best quality any of its episode streams advertised.
    pub quality: Option<&'a str>,
    /// The provider's own score out of ten.
    pub rating: Option<f32>,
    /// The provider's plot summary.
    pub overview: Option<&'a str>,
    /// The provider's genres, already split. Stored as JSON, which is the shape
    /// enrichment writes and `parse_genres` reads.
    pub genres: &'a [String],
    /// When the provider last touched this, in unix seconds — `get_series` sends no
    /// added date, and this is the closest thing it has.
    pub added_at: Option<i64>,
}

pub fn upsert_series(
    conn: &mut Connection,
    provider_id: i64,
    series: &NewSeries<'_>,
    now: i64,
) -> Result<i64> {
    let NewSeries {
        provider_key,
        title,
        match_key,
        year,
        poster,
        group,
        quality,
        rating,
        overview,
        genres,
        added_at,
    } = *series;
    // Empty stays NULL rather than becoming `[]`: the recommender's candidate query
    // treats both as "no genres", and NULL is what an un-enriched row already reads as.
    let genres_json = if genres.is_empty() {
        None
    } else {
        Some(serde_json::to_string(genres)?)
    };
    conn.execute(
        r#"INSERT INTO series (provider_id, provider_key, title, match_key, year, poster,
                               group_title, quality, rating, overview, genres,
                               added_at, last_seen_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
           ON CONFLICT (provider_id, provider_key) DO UPDATE SET
             title = excluded.title, match_key = excluded.match_key,
             year = COALESCE(excluded.year, series.year),
             poster = COALESCE(excluded.poster, series.poster),
             group_title = COALESCE(excluded.group_title, series.group_title),
             quality = COALESCE(excluded.quality, series.quality),
             -- Existing wins on all three: enrichment may already have overwritten
             -- them with TMDB's, and a refresh must not undo that.
             rating = COALESCE(series.rating, excluded.rating),
             overview = COALESCE(series.overview, excluded.overview),
             genres = COALESCE(series.genres, excluded.genres),
             last_seen_at = excluded.last_seen_at"#,
        params![
            provider_id,
            provider_key,
            title,
            match_key,
            year,
            poster,
            group,
            quality,
            rating,
            overview,
            genres_json,
            added_at.unwrap_or(now),
            now
        ],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM series WHERE provider_id = ?1 AND provider_key = ?2",
        params![provider_id, provider_key],
        |r| r.get(0),
    )?)
}

pub fn upsert_episodes(
    conn: &mut Connection,
    series_id: i64,
    episodes: &[NewEpisode],
    now: i64,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            r#"INSERT INTO episodes (series_id, season, episode, title, url, still, added_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7)
               ON CONFLICT (series_id, season, episode) DO UPDATE SET
                 title = COALESCE(excluded.title, episodes.title),
                 url   = excluded.url,
                 still = COALESCE(excluded.still, episodes.still)"#,
        )?;
        for e in episodes {
            stmt.execute(params![
                series_id, e.season, e.episode, e.title, e.url, e.still, now
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeRow {
    pub id: i64,
    pub season: u16,
    pub episode: u16,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub still: Option<String>,
    pub runtime_mins: Option<u32>,
    pub url: String,
}

/// Which show an episode belongs to.
///
/// Continue Watching stores progress against the *episode*, but what belongs on the
/// home screen is the show — one card reading "you are partway through this", not one
/// card per episode of it.
pub fn series_of_episode(conn: &Connection, episode_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT series_id FROM episodes WHERE id = ?1",
            params![episode_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn episodes_for(
    conn: &Connection,
    series_id: i64,
    season: Option<u16>,
) -> Result<Vec<EpisodeRow>> {
    let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<EpisodeRow> {
        Ok(EpisodeRow {
            id: r.get(0)?,
            season: r.get::<_, i64>(1)? as u16,
            episode: r.get::<_, i64>(2)? as u16,
            title: r.get(3)?,
            overview: r.get(4)?,
            still: r.get(5)?,
            runtime_mins: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            url: r.get(7)?,
        })
    };
    let rows = match season {
        Some(s) => {
            let mut stmt = conn.prepare(
                "SELECT id, season, episode, title, overview, still, runtime_mins, url
                 FROM episodes WHERE series_id = ?1 AND season = ?2 ORDER BY episode",
            )?;
            let out = stmt
                .query_map(params![series_id, s], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            out
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, season, episode, title, overview, still, runtime_mins, url
                 FROM episodes WHERE series_id = ?1 ORDER BY season, episode",
            )?;
            let out = stmt
                .query_map(params![series_id], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            out
        }
    };
    Ok(rows)
}

/// The episode after this one, crossing a season boundary when needed.
///
/// This is deliberately *not* `progress::next_episode`, which finds the next
/// **unwatched** episode for a Continue Watching rail. The Next Episode button has to
/// go to the literal next one even if the viewer has seen it.
pub fn following_episode(conn: &Connection, episode_id: i64) -> Result<Option<EpisodeRow>> {
    let Some((series_id, season, episode)) = conn
        .query_row(
            "SELECT series_id, season, episode FROM episodes WHERE id = ?1",
            params![episode_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };

    Ok(conn
        .query_row(
            "SELECT id, season, episode, title, overview, still, runtime_mins, url
             FROM episodes
             WHERE series_id = ?1
               AND (season > ?2 OR (season = ?2 AND episode > ?3))
             ORDER BY season, episode
             LIMIT 1",
            params![series_id, season, episode],
            |r| {
                Ok(EpisodeRow {
                    id: r.get(0)?,
                    season: r.get::<_, i64>(1)? as u16,
                    episode: r.get::<_, i64>(2)? as u16,
                    title: r.get(3)?,
                    overview: r.get(4)?,
                    still: r.get(5)?,
                    runtime_mins: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
                    url: r.get(7)?,
                })
            },
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    /// Continue Watching stores progress against an episode and shows the *show*, so
    /// this lookup is what turns one into the other. A wrong answer here puts the
    /// wrong poster on the home screen.
    #[test]
    fn an_episode_names_the_show_it_belongs_to() {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
             VALUES (1, 'P', 'm3u', 'http://e.com', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO series (id, provider_id, provider_key, title, match_key, last_seen_at)
             VALUES (7, 1, 'series:7', 'A Show', 'ashow', 0),
                    (8, 1, 'series:8', 'Another', 'another', 0)",
            [],
        )
        .unwrap();
        upsert_episodes(
            &mut conn,
            7,
            &[NewEpisode {
                season: 2,
                episode: 4,
                title: Some("Fourth".into()),
                url: "u".into(),
                still: None,
            }],
            0,
        )
        .unwrap();

        let episode = episodes_for(&conn, 7, None).unwrap()[0].id;
        assert_eq!(series_of_episode(&conn, episode).unwrap(), Some(7));

        // A progress row outlives the episode a refresh removed, so this has to answer
        // rather than fail — the caller drops the card.
        assert_eq!(series_of_episode(&conn, 9_999).unwrap(), None);
    }

    #[test]
    fn stats_on_an_empty_library_are_all_zero() {
        let conn = crate::open_memory().unwrap();
        let s = stats(&conn).unwrap();
        assert_eq!((s.channels, s.movies, s.series, s.episodes), (0, 0, 0, 0));
        assert_eq!(s.epg_coverage.matched, 0);
        assert!(s.epg_coverage.unmatched.is_empty());
    }

    #[test]
    fn coverage_counts_channels_with_a_guide_not_programmes() {
        // A guide that imported ten thousand programmes and matched nothing looks
        // exactly like a missing one from the sofa, so this counts the channels.
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
             VALUES (1, 'P', 'm3u', 'http://e.com', 0)",
            [],
        )
        .unwrap();
        for (key, epg) in [("a", Some("one")), ("b", None), ("c", None)] {
            conn.execute(
                "INSERT INTO channels (provider_id, provider_key, name, match_key,
                                       epg_channel_id, last_seen_at)
                 VALUES (1, ?1, ?1, ?1, ?2, 0)",
                rusqlite::params![key, epg],
            )
            .unwrap();
        }

        let s = stats(&conn).unwrap();
        assert_eq!(s.channels, 3);
        assert_eq!(s.epg_coverage.total, 3);
        assert_eq!(s.epg_coverage.matched, 1);
        assert_eq!(s.epg_coverage.unmatched, vec!["b", "c"]);
    }

    #[test]
    fn an_empty_string_is_not_a_match() {
        // Some providers write tvg-id="" rather than omitting it, and counting that as
        // matched would report full coverage for a guide that never arrived.
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
             VALUES (1, 'P', 'm3u', 'http://e.com', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (provider_id, provider_key, name, match_key,
                                   epg_channel_id, last_seen_at)
             VALUES (1, 'a', 'A', 'a', '', 0)",
            [],
        )
        .unwrap();

        assert_eq!(stats(&conn).unwrap().epg_coverage.matched, 0);
    }

    use super::*;

    fn provider(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO providers (name, kind, base_url, created_at)
             VALUES ('P','xtream','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// The panel in docs/ROADMAP.md scores 109,999 of its 122,499 films and dates all
    /// 122,499, and the import used to parse both and drop them. That left `rating`
    /// NULL on every row of a library with no TMDB key — which is most libraries —
    /// and `added_at` set to the moment of the import, so Browse's default
    /// "Recently added" order was sorting by a constant.
    #[test]
    fn a_films_own_score_and_date_are_kept() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let m = NewMovie {
            rating: Some(6.97),
            added_at: Some(1_784_596_062),
            ..movie("m1", "Some Film", Some(2020))
        };
        upsert_movies(&mut conn, p, &[m], 99).unwrap();

        let (rating, added): (Option<f64>, i64) = conn
            .query_row("SELECT rating, added_at FROM movies", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(rating, Some(6.97_f32 as f64));
        assert_eq!(
            added, 1_784_596_062,
            "dated by the import, not by the provider"
        );
    }

    /// A refresh must not undo enrichment. TMDB's score is the better one and it is
    /// written *after* the import that would otherwise overwrite it every time.
    #[test]
    fn a_refresh_leaves_an_enriched_rating_alone() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let from_panel = || NewMovie {
            rating: Some(6.9),
            ..movie("m1", "Some Film", None)
        };
        upsert_movies(&mut conn, p, &[from_panel()], 0).unwrap();
        conn.execute("UPDATE movies SET rating = 8.4", []).unwrap(); // enrichment ran
        upsert_movies(&mut conn, p, &[from_panel()], 1).unwrap();

        let rating: Option<f64> = conn
            .query_row("SELECT rating FROM movies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            rating,
            Some(8.4),
            "a playlist refresh undid what TMDB found"
        );
    }

    /// And where enrichment never ran, the provider's fills the gap on the next
    /// refresh rather than leaving the column empty for good.
    #[test]
    fn a_refresh_fills_a_rating_nothing_else_supplied() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(&mut conn, p, &[movie("m1", "Some Film", None)], 0).unwrap();
        upsert_movies(
            &mut conn,
            p,
            &[NewMovie {
                rating: Some(7.1),
                ..movie("m1", "Some Film", None)
            }],
            1,
        )
        .unwrap();

        let rating: Option<f64> = conn
            .query_row("SELECT rating FROM movies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rating, Some(7.1_f32 as f64));
    }

    /// Genres are the recommender's heaviest signal and came only from TMDB, so a
    /// library without an API key had none at all. The panel sends them for 27,661 of
    /// its 28,716 shows — stored in the same JSON shape enrichment writes, so the
    /// candidate query matches either source without knowing which it got.
    #[test]
    fn a_shows_metadata_is_stored_the_way_the_recommender_looks_it_up() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let genres = vec!["Crime".to_string(), "Drama".to_string()];
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Some Show",
                match_key: "someshow",
                rating: Some(9.0),
                overview: Some("A plot."),
                genres: &genres,
                added_at: Some(1_741_614_903),
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let (raw, overview, rating, added): (String, String, f64, i64) = conn
            .query_row(
                "SELECT genres, overview, rating, added_at FROM series",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(raw, r#"["Crime","Drama"]"#);
        assert_eq!(overview, "A plot.");
        assert_eq!(rating, 9.0);
        assert_eq!(added, 1_741_614_903);

        // Exactly the predicate `repo::recommend::candidates` builds.
        let found: i64 = conn
            .query_row(
                r#"SELECT count(*) FROM series
                    WHERE LOWER(genres) LIKE '%"' || ?1 || '"%'"#,
                ["crime"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            found, 1,
            "the recommender cannot find this show by its genre"
        );
    }

    /// No genres stays NULL rather than becoming `[]`. Both read as "unknown"
    /// downstream, and NULL is already what an un-enriched row holds — two spellings
    /// of the same thing is how a query ends up missing one of them.
    #[test]
    fn a_show_with_no_genres_stores_nothing_rather_than_an_empty_list() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Some Show",
                match_key: "someshow",
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let genres: Option<String> = conn
            .query_row("SELECT genres FROM series", [], |r| r.get(0))
            .unwrap();
        assert_eq!(genres, None);
    }

    /// Films and shows do not share a genre list, and offering one the other's is a
    /// filter that matches nothing.
    ///
    /// Invisible while genres came only from TMDB and most libraries had none. The
    /// moment the provider's own were kept (D26) it became the Movies screen showing
    /// a dropdown of 28,529 shows' genres on a panel that sends no film genre at all.
    #[test]
    fn each_half_of_the_library_offers_only_its_own_genres() {
        use crate::repo::filtering::Kind;
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);

        upsert_movies(&mut conn, p, &[movie("m1", "Some Film", None)], 0).unwrap();
        conn.execute(r#"UPDATE movies SET genres = '["Western"]'"#, [])
            .unwrap();
        let shows = vec!["Crime".to_string(), "Drama".to_string()];
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Some Show",
                match_key: "someshow",
                genres: &shows,
                ..Default::default()
            },
            0,
        )
        .unwrap();

        assert_eq!(genres(&conn, Kind::Movies).unwrap(), vec!["Western"]);
        assert_eq!(genres(&conn, Kind::Series).unwrap(), vec!["Crime", "Drama"]);
    }

    /// A hidden row is not on the screen, so its genres are not in the filter either —
    /// a filter offering a value that yields nothing is the complaint this whole
    /// facet exists to answer.
    #[test]
    fn a_hidden_rows_genres_are_not_offered() {
        use crate::repo::filtering::Kind;
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(&mut conn, p, &[movie("m1", "Some Film", None)], 0).unwrap();
        conn.execute(
            r#"UPDATE movies SET genres = '["Western"]', hidden = 1"#,
            [],
        )
        .unwrap();
        assert!(genres(&conn, Kind::Movies).unwrap().is_empty());
    }

    fn movie(key: &str, title: &str, year: Option<i32>) -> NewMovie {
        NewMovie {
            provider_key: key.into(),
            title: title.into(),
            match_key: aurora_core::title::match_key(title),
            year,
            url: format!("https://example.com/{key}.mkv"),
            ..Default::default()
        }
    }

    #[test]
    fn upserts_and_sorts_movies() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(
            &mut conn,
            p,
            &[
                movie("1", "Zulu", Some(1964)),
                movie("2", "Alien", Some(1979)),
            ],
            100,
        )
        .unwrap();

        let by_title = list_movies(
            &conn,
            &BrowseQuery {
                sort: MovieSort::Title,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_title[0].title, "Alien");

        let by_year = list_movies(
            &conn,
            &BrowseQuery {
                sort: MovieSort::Year,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_year[0].title, "Alien");
    }

    #[test]
    fn refresh_does_not_wipe_enriched_metadata() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(&mut conn, p, &[movie("1", "Alien", Some(1979))], 100).unwrap();

        // Simulate TMDB enrichment writing fields the playlist never supplies.
        conn.execute(
            "UPDATE movies SET overview = 'In space...', backdrop = 'b.jpg', rating = 8.4",
            [],
        )
        .unwrap();

        upsert_movies(&mut conn, p, &[movie("1", "Alien (1979)", Some(1979))], 200).unwrap();

        let m = &list_movies(
            &conn,
            &BrowseQuery {
                sort: MovieSort::Title,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap()[0];
        assert_eq!(m.overview.as_deref(), Some("In space..."));
        assert_eq!(m.backdrop.as_deref(), Some("b.jpg"));
        assert_eq!(m.rating, Some(8.4));
        assert_eq!(
            m.title, "Alien (1979)",
            "provider title should still update"
        );
    }

    #[test]
    fn duplicates_are_detectable_across_providers() {
        let mut conn = crate::open_memory().unwrap();
        let p1 = provider(&conn);
        let p2 = provider(&conn);
        upsert_movies(&mut conn, p1, &[movie("a", "Alien", Some(1979))], 0).unwrap();
        upsert_movies(&mut conn, p2, &[movie("b", "ALIEN 1080p", Some(1979))], 0).unwrap();

        let dupes = duplicate_groups(&conn).unwrap();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].1, 2);
    }

    #[test]
    fn series_and_episodes_round_trip() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Breaking Bad",
                match_key: "breakingbad",
                year: Some(2008),
                poster: None,
                group: None,
                quality: None,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        upsert_episodes(
            &mut conn,
            sid,
            &[
                NewEpisode {
                    season: 1,
                    episode: 2,
                    title: Some("Two".into()),
                    url: "u2".into(),
                    still: None,
                },
                NewEpisode {
                    season: 1,
                    episode: 1,
                    title: Some("One".into()),
                    url: "u1".into(),
                    still: None,
                },
                NewEpisode {
                    season: 2,
                    episode: 1,
                    title: None,
                    url: "u3".into(),
                    still: None,
                },
            ],
            0,
        )
        .unwrap();

        let all = episodes_for(&conn, sid, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!((all[0].season, all[0].episode), (1, 1));
        assert_eq!((all[2].season, all[2].episode), (2, 1));

        let s1 = episodes_for(&conn, sid, Some(1)).unwrap();
        assert_eq!(s1.len(), 2);
    }

    #[test]
    fn reimporting_an_episode_updates_rather_than_duplicating() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let ep = NewEpisode {
            season: 1,
            episode: 1,
            title: Some("T".into()),
            url: "old".into(),
            still: None,
        };
        upsert_episodes(&mut conn, sid, std::slice::from_ref(&ep), 0).unwrap();
        upsert_episodes(
            &mut conn,
            sid,
            &[NewEpisode {
                url: "new".into(),
                title: None,
                ..ep
            }],
            0,
        )
        .unwrap();

        let rows = episodes_for(&conn, sid, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "new");
        assert_eq!(
            rows[0].title.as_deref(),
            Some("T"),
            "title should not be nulled"
        );
    }

    #[test]
    fn following_episode_walks_the_season_then_crosses_into_the_next() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        upsert_episodes(
            &mut conn,
            sid,
            &[
                NewEpisode {
                    season: 1,
                    episode: 1,
                    url: "a".into(),
                    ..Default::default()
                },
                NewEpisode {
                    season: 1,
                    episode: 2,
                    url: "b".into(),
                    ..Default::default()
                },
                NewEpisode {
                    season: 2,
                    episode: 1,
                    url: "c".into(),
                    ..Default::default()
                },
            ],
            0,
        )
        .unwrap();

        let eps = episodes_for(&conn, sid, None).unwrap();
        let next = following_episode(&conn, eps[0].id).unwrap().unwrap();
        assert_eq!((next.season, next.episode), (1, 2));

        // Crosses the season boundary.
        let across = following_episode(&conn, eps[1].id).unwrap().unwrap();
        assert_eq!((across.season, across.episode), (2, 1));

        // The last episode has no successor.
        assert!(following_episode(&conn, eps[2].id).unwrap().is_none());
    }

    #[test]
    fn following_episode_handles_gaps_and_unknown_ids() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        // A provider that is missing episode 2 entirely.
        upsert_episodes(
            &mut conn,
            sid,
            &[
                NewEpisode {
                    season: 1,
                    episode: 1,
                    url: "a".into(),
                    ..Default::default()
                },
                NewEpisode {
                    season: 1,
                    episode: 5,
                    url: "b".into(),
                    ..Default::default()
                },
            ],
            0,
        )
        .unwrap();

        let eps = episodes_for(&conn, sid, None).unwrap();
        let next = following_episode(&conn, eps[0].id).unwrap().unwrap();
        assert_eq!(next.episode, 5, "should skip the gap, not stop at it");

        assert!(following_episode(&conn, 99_999).unwrap().is_none());
    }

    #[test]
    fn series_upsert_is_stable_across_refreshes() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let b = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Show HD",
                match_key: "show",
                year: Some(2020),
                poster: None,
                group: None,
                quality: None,
                ..Default::default()
            },
            1,
        )
        .unwrap();
        assert_eq!(a, b, "same provider_key must map to the same row");
    }
}
